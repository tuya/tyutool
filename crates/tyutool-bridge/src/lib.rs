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
//! B7 scope: local-transport hardening — `Origin` is a browser-supplied header
//! and therefore no defence against a native local process, so the dangerous
//! operations (`run_job` / `run_auth`) sit behind a human confirmation and that
//! one click is persisted as an Origin-bound token which a later connection
//! presents as `?token=` on the handshake (a token the store does not recognize
//! only downgrades the connection, it never refuses it); grants live in
//! `{config_dir}/tyutool-bridge/grants.json` (0600), never expire, and are
//! cleared by revoking. The gate re-checks that token against the store on
//! every dangerous operation rather than trusting a handshake-time verdict, so
//! revoking takes a connection's privilege away immediately instead of only for
//! the connections that come after it. Defence in depth on top of that click: at most one
//! dangerous operation runs process-wide at a time (confirmation dialog
//! included), and every one of them leaves exactly one audit line. Everything
//! low-risk (hello / ports / serial monitor) stays open so "插线即就绪" survives.

pub mod lang;
pub mod status;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// Local development addresses (cobuilder-web dev server defaults to port
/// 3000; 5173 is the plain Vite default kept as fallback) plus every origin a
/// real cobuilder-web deployment is served from, transcribed from cobuilder-web
/// `config/index.cjs` (`base` / `daily` / `pre` / `prod` region maps). `base`
/// and `daily` both serve from `dev-claw-wb.wgine.com`, and `pre` maps both AZ
/// and SG to `developer-us.wgine.com`, so each of those appears once.
///
/// ## Missing an origin is a release-level mistake, not a config typo
///
/// This is a **compile-time** constant: it ships inside the installed binary.
/// An origin left out of this list cannot be fixed by editing a config file on
/// a user's machine — the only remedy is to cut a new Bridge release **and get
/// every existing user to reinstall**, because their installed binary will keep
/// answering 403 forever. The first person to open cobuilder-web on the missing
/// origin just sees the connection fail. So when a new region or environment
/// appears, add it here *before* it ships, and prefer to over-include a legit
/// cobuilder-web origin over discovering it after release.
///
/// ## Still: exact matching only
///
/// Entries are compared **byte-for-byte** by [`allowlisted_origin`]. Never
/// relax that into wildcard or suffix matching: `*.wgine.com` would hand the
/// bridge — including the dangerous flash / auth operations behind it — to
/// anyone who takes over a sibling subdomain. The reinstall cost above is the
/// reason to be thorough about enumerating origins, never a reason to reach for
/// a pattern. New regions are added here as literals.
pub const ORIGIN_ALLOWLIST: &[&str] = &[
    // Local development.
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
    // Daily (internal test environment; `base` and `daily` share this origin).
    // The team validates flashing here, so it must ship in the allowlist.
    "https://dev-claw-wb.wgine.com",
    // Pre-release (wgine).
    "https://developer.wgine.com",
    "https://developer-us.wgine.com",
    "https://developer-eu.wgine.com",
    "https://developer-in.wgine.com",
    "https://developer-ue.wgine.com",
    "https://developer-we.wgine.com",
    // Production (tuya).
    "https://platform.tuya.com",
    "https://us.platform.tuya.com",
    "https://eu.platform.tuya.com",
    "https://ind.platform.tuya.com",
    "https://ue.platform.tuya.com",
    "https://we.platform.tuya.com",
    "https://sg.platform.tuya.com",
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
    /// USB iSerial string. The two UART bridges of one dual-serial board report
    /// the **same** value, which is what lets a client group ports into devices
    /// instead of showing one board twice. `None` for non-USB ports and for
    /// devices that ship no serial number.
    pub serial_number: Option<String>,
    /// USB interface number, the only thing that tells apart two ports sharing a
    /// `serial_number`.
    ///
    /// `None` is normal, not an error: Linux commonly omits it. The number is
    /// also **not comparable across platforms** — macOS reports the CDC data
    /// interfaces (1 / 3) where Windows reports the paired control interfaces
    /// (0 / 2) for the same board.
    pub usb_interface: Option<u8>,
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
///
/// `Debug` is hand-written (see the impl below): a derive would print `uuid` and
/// `auth_key` in full.
#[derive(Clone)]
pub struct AuthJobSpec {
    pub chip_id: String,
    pub port: String,
    pub baud_rate: u32,
    pub uuid: String,
    pub auth_key: String,
}

/// A secret rendered for `Debug`: presence and length only.
///
/// Not a prefix, not a hash — an authorization uuid is short enough that any
/// leading fragment narrows it usefully, and a log line only ever needs to
/// answer "was a credential there at all, and did it look empty?".
struct Redacted<'a>(&'a str);

impl std::fmt::Debug for Redacted<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            f.write_str("<empty>")
        } else {
            write!(f, "<redacted len={}>", self.0.len())
        }
    }
}

/// A base64 payload rendered for `Debug`: its size only.
///
/// A firmware image is multi-MB of base64; dumping it into a log line is a
/// denial of service against whoever has to read that log.
struct Base64Len(usize);

impl std::fmt::Debug for Base64Len {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<base64 len={}>", self.0)
    }
}

/// Hand-written so no future `{spec:?}` can leak a credential — the same
/// compile-time guarantee [`ConfirmRequest`] gets by carrying no secret at all.
impl std::fmt::Debug for AuthJobSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthJobSpec")
            .field("chip_id", &self.chip_id)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .field("uuid", &Redacted(&self.uuid))
            .field("auth_key", &Redacted(&self.auth_key))
            .finish()
    }
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

// ── Local-transport hardening (B7) ───────────────────────────────────────────
//
// The trust anchor is the *user's click*, not a token: `Origin` is a header the
// browser is forced to add, so any native local process can present an
// allowlisted one and complete the handshake. A token merely persists one human
// confirmation so the user is not asked again.

/// An operation that can damage the device, so it needs a human confirmation:
/// flashing overwrites the firmware, authorizing overwrites a code that cannot
/// be restored (PRD: 覆盖不可撤销).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerousOp {
    Flash,
    Authorize,
}

impl DangerousOp {
    /// Stable label for logs and audit lines.
    pub fn as_str(self) -> &'static str {
        match self {
            DangerousOp::Flash => "flash",
            DangerousOp::Authorize => "authorize",
        }
    }
}

/// What the confirmation UI has to show the user before they can consent.
#[derive(Debug, Clone)]
pub struct ConfirmRequest {
    pub op: DangerousOp,
    /// The allowlisted `Origin` of the asking connection.
    pub origin: String,
    pub chip_id: String,
    pub port: String,
    /// Decoded firmware size for [`DangerousOp::Flash`], `None` for an
    /// authorization write (which carries no image) and for a firmware payload
    /// whose base64 is malformed.
    pub firmware_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDecision {
    Approve,
    Reject,
}

/// Answer channel handed to the confirmation UI; callable exactly once, from
/// any thread.
pub type ConfirmResponder = Box<dyn FnOnce(ConfirmDecision) + Send>;

/// Human-in-the-loop gate. Implementations must NOT block the caller: the
/// decision arrives through `respond`, callable from any thread.
pub trait AuthPrompt: Send + Sync {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder);
}

/// One persisted confirmation: the user said yes once, and this is the receipt.
///
/// `Debug` is hand-written (see the impl below) for the same reason
/// [`AuthJobSpec`]'s is: `token` is a bearer credential — presenting it is
/// enough to write a device — so a derive would let one `log::debug!("{grant:?}")`
/// or a `Vec<Grant>` interpolated into an error message dump it whole.
#[derive(Clone, PartialEq, Eq)]
pub struct Grant {
    pub token: String,
    pub origin: String,
    pub granted_at_ms: u64,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            // Deliberately [`Redacted`] and not [`redact_token`]: the
            // fingerprint's leading characters exist so two *audit* lines can be
            // correlated, and a Debug rendering has no such need — so it takes
            // the strict form.
            .field("token", &Redacted(&self.token))
            .field("origin", &self.origin)
            .field("granted_at_ms", &self.granted_at_ms)
            .finish()
    }
}

/// Whether a connection may lean on a grant persisted by an earlier session.
///
/// The distinction exists because one `grants.json` is shared by every run mode:
/// a grant records that **a human confirmed this at a keyboard**, and that
/// consent must not silently extend into a session where no human is present.
/// `--headless` without `--allow-unattended-writes` therefore runs with
/// [`GrantPolicy::Ignore`] — the existence of an explicit opt-in switch is
/// itself the statement that unattended operation has to be declared, so the
/// safe default wins over the convenience of skipping a dialog nobody can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantPolicy {
    /// A `?token=` the store still grants for this Origin authorizes dangerous
    /// operations without a dialog. The attended default.
    Honour,
    /// Persisted grants are ignored entirely; only a confirmation clicked *in
    /// this connection* authorizes anything.
    Ignore,
}

/// Where issued grants live. Injectable so the production process can persist
/// them while tests keep them in memory.
pub trait TokenStore: Send + Sync {
    fn is_granted(&self, token: &str, origin: &str) -> bool;
    fn insert(&self, grant: Grant);
    /// Drop every stored grant and report how many there were, which is the
    /// `grants=<n>` of the [`audit_revoke_all`] line — only the store can count
    /// them, and a revocation that is not auditable is not much of a control.
    fn revoke_all(&self) -> usize;
}

/// Library default: grants live for the lifetime of the process only.
#[derive(Default)]
pub struct MemoryTokenStore {
    grants: Mutex<Vec<Grant>>,
}

impl MemoryTokenStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Grant>> {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TokenStore for MemoryTokenStore {
    fn is_granted(&self, token: &str, origin: &str) -> bool {
        self.lock()
            .iter()
            .any(|grant| grant.token == token && grant.origin == origin)
    }

    fn insert(&self, grant: Grant) {
        self.lock().push(grant);
    }

    fn revoke_all(&self) -> usize {
        let mut grants = self.lock();
        let count = grants.len();
        grants.clear();
        count
    }
}

/// On-disk grant file layout version. Bump only on a breaking change; an
/// unrecognized version is treated exactly like a corrupt file (start empty).
const GRANT_FILE_VERSION: u32 = 1;

/// Wire mirror of [`Grant`], deliberately private: the on-disk JSON layout is
/// this crate's business, so it must be free to move without serde derives (and
/// therefore a serialized representation) leaking onto the public API type.
#[derive(Serialize, Deserialize)]
struct WireGrant {
    token: String,
    origin: String,
    granted_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct GrantFile {
    version: u32,
    grants: Vec<WireGrant>,
}

/// Persistent [`TokenStore`]: JSON at `{config_dir}/tyutool-bridge/grants.json`,
/// mode 0600 on unix.
///
/// It exists so a reboot does not cost the user another confirmation. Grants
/// carry **no expiry on purpose** — revocation ("撤销所有授权") is the mechanism;
/// a clock would only re-ask the user on a schedule nobody chose.
///
/// Reads are served from the in-memory copy: `is_granted` sits on the WS
/// handshake path *and* on the gate of every dangerous operation
/// (`ConnContext::is_authorized` re-checks the presented token there), so it
/// must never touch the disk. Writes go through to the file.
pub struct FileTokenStore {
    path: std::path::PathBuf,
    grants: Mutex<Vec<Grant>>,
}

impl FileTokenStore {
    /// Production location: `{config_dir}/tyutool-bridge/grants.json`.
    ///
    /// The *config* dir, not the data dir the session logs live in: a grant is
    /// user configuration ("this origin may flash"), not a diagnostic artefact.
    pub fn open() -> anyhow::Result<Self> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("no platform config directory"))?
            .join("tyutool-bridge");
        Self::open_at(&dir.join("grants.json"))
    }

    /// Load (or start) the grant file at `path`, creating missing parents.
    ///
    /// A missing file means "no grants yet". A file that cannot be read or
    /// parsed is **not** fatal: this runs inside a resident helper, and refusing
    /// to start would cost the user the whole bridge over a file whose entire
    /// content is a convenience. Such a file degrades to an empty set (so the
    /// user is asked again) and the next grant overwrites it.
    pub fn open_at(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("create {}: {e}", parent.display()))?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            grants: Mutex::new(load_grants(path)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Grant>> {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TokenStore for FileTokenStore {
    fn is_granted(&self, token: &str, origin: &str) -> bool {
        self.lock()
            .iter()
            .any(|grant| grant.token == token && grant.origin == origin)
    }

    fn insert(&self, grant: Grant) {
        let mut grants = self.lock();
        log::info!(
            "bridge persisting a grant (origin={}, token={})",
            grant.origin,
            redact_token(&grant.token)
        );
        grants.push(grant);
        persist_grants(&self.path, &grants);
    }

    fn revoke_all(&self) -> usize {
        let mut grants = self.lock();
        let count = grants.len();
        grants.clear();
        // Clears the *file* as well: a revocation that only emptied memory would
        // silently come back on the next restart.
        persist_grants(&self.path, &grants);
        log::info!("bridge revoked {count} persisted grant(s)");
        count
    }
}

/// Grants currently on disk, or an empty set for anything unreadable.
///
/// Never logs the file's contents — it is a list of secrets.
fn load_grants(path: &std::path::Path) -> Vec<Grant> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!(
                "bridge cannot read the grant file {} ({e}), starting with no grants",
                path.display()
            );
            return Vec::new();
        }
    };
    let file: GrantFile = match serde_json::from_str(&text) {
        Ok(file) => file,
        Err(e) => {
            log::warn!(
                "bridge grant file {} is not parsable ({e}), starting with no grants",
                path.display()
            );
            return Vec::new();
        }
    };
    if file.version != GRANT_FILE_VERSION {
        log::warn!(
            "bridge grant file {} has unsupported version {}, starting with no grants",
            path.display(),
            file.version
        );
        return Vec::new();
    }
    file.grants
        .into_iter()
        .map(|wire| Grant {
            token: wire.token,
            origin: wire.origin,
            granted_at_ms: wire.granted_at_ms,
        })
        .collect()
}

/// Replace the grant file with `grants`.
///
/// Failure is logged, never propagated: the in-memory copy is already updated,
/// so the worst case is one extra confirmation after the next restart — and a
/// resident helper must not die because a config directory went read-only.
fn persist_grants(path: &std::path::Path, grants: &[Grant]) {
    if let Err(e) = write_grants(path, grants) {
        log::warn!(
            "bridge could not persist grants to {}: {e:#}",
            path.display()
        );
    }
}

/// Write the grant list through a sibling temp file and rename it over the
/// target: a crash mid-write must not be able to turn a good file into a
/// truncated or empty one.
fn write_grants(path: &std::path::Path, grants: &[Grant]) -> anyhow::Result<()> {
    use std::io::Write;

    let file = GrantFile {
        version: GRANT_FILE_VERSION,
        grants: grants
            .iter()
            .map(|grant| WireGrant {
                token: grant.token.clone(),
                origin: grant.origin.clone(),
                granted_at_ms: grant.granted_at_ms,
            })
            .collect(),
    };
    let text = serde_json::to_string(&file)?;

    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("{} has no file name", path.display()))?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);

    {
        // The temp file carries the final permissions, so the token is never
        // even briefly world-readable (the rename below preserves them).
        let mut handle = create_private(&tmp)?;
        handle.write_all(text.as_bytes())?;
        handle.flush()?;
    }
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

/// Create/truncate `path` for writing, readable and writable by its owner only.
#[cfg(unix)]
fn create_private(path: &std::path::Path) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    // `mode` applies only when *this* call created the file, so a leftover from
    // an interrupted write would keep whatever permissions it had. Narrow it
    // before any token reaches the disk, not after.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| anyhow::anyhow!("chmod {}: {e}", path.display()))?;
    Ok(file)
}

/// Windows has no POSIX mode bits (and no chmod): the grant file inherits the
/// ACL of the per-user config directory under `%APPDATA%`, which is already
/// restricted to the account that owns it. A deliberate documented no-op rather
/// than a permission change that would only pretend to do something.
#[cfg(not(unix))]
fn create_private(path: &std::path::Path) -> anyhow::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))
}

/// Where the "who asked for what, and what did the user answer" trail goes.
pub trait AuditSink: Send + Sync {
    fn record(&self, line: &str);
}

/// Library default: the audit trail joins the developer log channel under its
/// own target, so it can be filtered out of (or into) a dedicated sink.
pub struct LogAuditSink;

impl AuditSink for LogAuditSink {
    fn record(&self, line: &str) {
        log::info!(target: "bridge::audit", "{line}");
    }
}

/// Library default prompt: refuses immediately and says so loudly.
///
/// Deliberately inert rather than "ask the OS": a process that forgot to inject
/// a real confirmation UI must fail visibly, and `cargo test` must never pop a
/// dialog.
struct DenyPrompt;

impl AuthPrompt for DenyPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        log::error!(
            "bridge has no confirmation UI wired, refusing {} on {} from {}",
            request.op.as_str(),
            request.port,
            request.origin
        );
        respond(ConfirmDecision::Reject);
    }
}

/// How long a confirmation dialog may stay unanswered before the operation is
/// refused, and the default of [`BridgeServer::with_confirm_timeout`].
///
/// Long enough for a user who has to walk back to the machine, short enough
/// that a dialog nobody will ever answer does not pin a job task forever.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);

/// The injected security surface, bundled so one `Arc` threads through the
/// accept loop into every connection.
struct SecurityConfig {
    prompt: Arc<dyn AuthPrompt>,
    tokens: Arc<dyn TokenStore>,
    audit: Arc<dyn AuditSink>,
    confirm_timeout: Duration,
    /// Whether persisted grants count at all; see [`GrantPolicy`].
    grant_policy: GrantPolicy,
    /// Every live connection, so a revocation can reach the ones already open and
    /// not just the grant file. Shared with the [`Authority`] handles handed out
    /// before `run_*` consumes the server.
    connections: Arc<ConnectionRegistry>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            prompt: Arc::new(DenyPrompt),
            tokens: Arc::new(MemoryTokenStore::default()),
            audit: Arc::new(LogAuditSink),
            confirm_timeout: CONFIRM_TIMEOUT,
            // Honouring grants is the attended behaviour every existing host and
            // test expects; ignoring them is the opt-in of the unattended
            // posture, not something a default may impose.
            grant_policy: GrantPolicy::Honour,
            connections: Arc::new(ConnectionRegistry::default()),
        }
    }
}

/// Fresh grant token: 32 CSPRNG bytes as base64url without padding (43 chars).
///
/// `None` means the OS gave us no entropy. Callers must refuse the operation
/// rather than fall back to a weaker source — a guessable token is worse than
/// asking the user again.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    match getrandom::fill(&mut bytes) {
        Ok(()) => Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)),
        Err(e) => {
            log::error!("bridge cannot issue a grant token, no CSPRNG entropy: {e}");
            None
        }
    }
}

/// The `token` query parameter of a handshake request URI, percent-decoded.
///
/// The web client builds the URL with an encoder, so the value arrives escaped;
/// decoding here is what keeps a credential from depending on which side
/// happened to escape it. A grant token is 43 base64url characters and needs no
/// escaping at all, but a token the *user* seeded or a future token alphabet
/// must not silently stop matching.
///
/// The first `token=` wins if a client repeats the parameter; an empty value
/// counts as absent, and so does one this crate cannot decode (see
/// [`percent_decode`]) — an unusable token downgrades the connection to
/// unauthorized, which is the safe direction.
fn token_from_query(query: Option<&str>) -> Option<String> {
    let raw = query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))?;
    let decoded = percent_decode(raw)?;
    (!decoded.is_empty()).then_some(decoded)
}

/// Percent-decode one URI **query value**.
///
/// Hand-rolled rather than a new dependency: this is the only percent-encoded
/// input the bridge has, and the whole rule is "`%XX` is a byte".
///
/// Two deliberate decisions:
/// - `+` stays a literal `+`. Form encoding (`application/x-www-form-urlencoded`)
///   reads it as a space, but this is a URI query value, not a form body — a
///   token containing `+` must round-trip unchanged.
/// - a malformed escape (`%`, `%A`, `%ZZ`) or a byte sequence that is not UTF-8
///   yields `None` instead of the raw text, so an undecodable token is treated
///   as no token at all (one more confirmation) rather than compared as
///   something the client never sent.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex_nibble(*bytes.get(i + 1)?)?;
            let low = hex_nibble(*bytes.get(i + 2)?)?;
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// One hex digit's value, either case; `None` for anything else.
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Token fingerprint for logs and audit lines: enough to correlate two entries,
/// never enough to replay the grant.
fn redact_token(token: &str) -> String {
    let head: String = token.chars().take(6).collect();
    format!("{head}…(len={})", token.len())
}

/// The one `confirm` line every dangerous operation leaves — whether a dialog
/// was shown, whether the connection was already authorized, or whether the
/// request was refused before it could ask anything.
///
/// Frozen format (see PROTOCOL.md §审计行): space-separated `key=value`, one line
/// per event, `-` for an absent value. `decision` carries the whole vocabulary:
/// `approved` / `rejected` / `timeout` / `abandoned` (a dialog was shown),
/// `preauthorized` (the connection already held a valid grant, so no dialog was
/// needed) and `execution_busy` (refused by the single-active-execution rule).
///
/// No credential can reach this line by construction: `uuid` / `auth_key` are
/// not part of [`ConfirmRequest`] at all, and tokens only ever appear through
/// [`redact_token`] on the separate `grant` line.
fn audit_confirm(audit: &dyn AuditSink, request: &ConfirmRequest, decision: &str) {
    audit.record(&format!(
        "confirm op={} origin={} chip={} port={} firmware_bytes={} decision={decision}",
        request.op.as_str(),
        request.origin,
        request.chip_id,
        request.port,
        match request.firmware_bytes {
            Some(bytes) => bytes.to_string(),
            None => "-".to_string(),
        },
    ));
}

/// The audit line a revocation leaves ("撤销所有授权" wipes every stored grant).
///
/// Public because the only caller is the shell around this library: the tray
/// menu item that revokes is B7 cycle 4's and lives in `main.rs`. Defined here
/// anyway so the whole audit vocabulary stays in one place, and so `grants=<n>`
/// is formatted the same way wherever a revocation happens.
pub fn audit_revoke_all(audit: &dyn AuditSink, grants: usize) {
    audit.record(&format!("revoke_all grants={grants}"));
}

/// The connections a revocation has to reach.
///
/// Held as `Weak` on purpose: the registry is a side index, not an owner, so a
/// connection that ended must be collectable even if its entry is still listed —
/// the alternative (strong references plus perfectly matched deregistration)
/// turns any missed teardown path into a leaked socket.
#[derive(Default)]
struct ConnectionRegistry {
    live: Mutex<Vec<Weak<ConnContext>>>,
}

impl ConnectionRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Weak<ConnContext>>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// List `ctx` until the returned guard is dropped.
    fn register(self: &Arc<Self>, ctx: &Arc<ConnContext>) -> RegisteredConnection {
        let mut live = self.lock();
        live.retain(|weak| weak.strong_count() > 0);
        live.push(Arc::downgrade(ctx));
        RegisteredConnection {
            registry: Arc::clone(self),
            conn_id: ctx.conn_id,
        }
    }

    /// The connections still alive right now.
    fn snapshot(&self) -> Vec<Arc<ConnContext>> {
        self.lock().iter().filter_map(Weak::upgrade).collect()
    }
}

/// Keeps one connection listed in the registry until dropped.
///
/// An RAII guard rather than a paired deregister call, for the same reason the
/// execution right is one: `handle_connection` has several exit paths, and a
/// forgotten one would leave a dead entry that `revoke_all` then walks.
struct RegisteredConnection {
    registry: Arc<ConnectionRegistry>,
    conn_id: u64,
}

impl Drop for RegisteredConnection {
    fn drop(&mut self) {
        let mut live = self.registry.lock();
        live.retain(|weak| match weak.upgrade() {
            Some(ctx) => ctx.conn_id != self.conn_id,
            None => false,
        });
    }
}

/// The revocation control the shell around this library drives: what the tray's
/// 「撤销所有授权」 item calls from the UI thread.
///
/// Cheap to clone and safe to hold across threads, so the server thread can hand
/// one to the UI thread and keep serving.
#[derive(Clone)]
pub struct Authority {
    tokens: Arc<dyn TokenStore>,
    audit: Arc<dyn AuditSink>,
    connections: Arc<ConnectionRegistry>,
}

// The trait objects behind it are not `Debug`, and a security handle has nothing
// worth printing anyway — but the host may well carry it inside a `Debug` event
// enum (the tray does), so the impl exists.
impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Authority")
    }
}

impl Authority {
    /// Withdraw every authorization: clear the store, drop the privilege of every
    /// connection that is already open, and tell each of them so.
    ///
    /// Clearing the store is what actually removes token-derived privilege —
    /// including on connections that are already open, because the gate re-reads
    /// the store on every dangerous operation (`ConnContext::is_authorized`).
    /// The walk over live connections covers the other half: an in-session dialog
    /// approval, which no store holds, plus the `auth_revoked` push that lets a
    /// client drop its stored token instead of discovering the revocation by
    /// wasting a flash attempt. So the registry is a completeness and
    /// notification mechanism here, not what makes revoking a token correct — a
    /// connection the walk misses is unprivileged all the same.
    ///
    /// Safe to call from any thread, including a UI thread: it takes no async
    /// runtime and never blocks on the network (frames are queued, not sent
    /// inline). The store write is a small local file.
    pub fn revoke_all(&self) {
        let grants = self.tokens.revoke_all();
        let live = self.connections.snapshot();
        for ctx in &live {
            ctx.approved
                .store(false, std::sync::atomic::Ordering::Relaxed);
            ctx.send(&ServerFrame::AuthRevoked);
        }
        audit_revoke_all(self.audit.as_ref(), grants);
        log::info!(
            "bridge revoked all authorizations: {grants} stored grant(s) cleared, \
             {} live connection(s) deauthorized",
            live.len()
        );
    }
}

/// Decoded length of a base64 payload, computed from the text alone.
///
/// The confirmation dialog names the firmware size, and it runs on the async
/// worker before anything is decoded — a multi-MB image must not be decoded
/// twice (nor decoded at all for an operation the user is about to refuse).
///
/// `None` for anything that is not well-formed padded base64; the caller still
/// asks (the job would fail `bad_request` on the real decode later anyway).
fn base64_decoded_len(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if !len.is_multiple_of(4) {
        return None;
    }
    if len == 0 {
        return Some(0);
    }
    let padding = bytes.iter().rev().take_while(|b| **b == b'=').count();
    // A quantum holds at most 2 pad characters, and they may only sit at the
    // very end.
    if padding > 2 || bytes[..len - padding].contains(&b'=') {
        return None;
    }
    Some((len / 4 * 3 - padding) as u64)
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
    /// Shared by every port of one physical device: the client's grouping key.
    #[serde(skip_serializing_if = "Option::is_none")]
    serial_number: Option<String>,
    /// Distinguishes ports that share a `serial_number`. Omitted when the OS
    /// does not report one (routine on Linux).
    #[serde(skip_serializing_if = "Option::is_none")]
    usb_interface: Option<u8>,
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
///
/// `Debug` is hand-written for the same reason as [`AuthJobSpec`]'s.
#[derive(Deserialize)]
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

impl std::fmt::Debug for WireAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WireAuth")
            .field("chip_id", &self.chip_id)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .field("uuid", &Redacted(&self.uuid))
            .field("auth_key", &Redacted(&self.auth_key))
            .finish()
    }
}

/// Authorization runs over the device's UART shell, not the flash bootloader,
/// so it uses the firmware console rate rather than the flash baud rate.
///
/// 115200 is that console rate for every chip we support: it is the value each
/// entry of `src/features/firmware-flash/chip-manifests.ts` carries as
/// `defaultAuthBaudRate`, and the rate the GUI batch pipeline, `tyutool-cli
/// authorize` and the direct-vendor web path all open the port at.
///
/// This used to be 921600 — T5AI's *flash* baud rate, copied over from the
/// flashing path by mistake. The web client deliberately omits `baud_rate` and
/// takes this default, so every browser-driven authorization was speaking to a
/// 115200 console at 921600: the device saw framing errors and dropped every
/// probe, never answering (`bytes=0` for the whole 30 s window). Measured on a
/// user's T5AI board on 2026-07-31 — within the same minute, the CLI at 115200
/// got its `tuya>` prompt 0.6 s after reset and finished reading the
/// authorization in 2.7 s.
fn default_auth_baud_rate() -> u32 {
    115_200
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

/// `Debug` is hand-written (see the impl below): a derive would print
/// [`WireAuth`]'s credentials through it and dump the whole base64 firmware
/// image of a `run_job` frame.
#[derive(Deserialize)]
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

impl std::fmt::Debug for ClientMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RunJob {
                request_id,
                job,
                file_content,
            } => f
                .debug_struct("RunJob")
                .field("request_id", request_id)
                .field("job", job)
                .field("file_content", &Base64Len(file_content.len()))
                .finish(),
            // `auth` redacts itself, see WireAuth's Debug impl.
            Self::RunAuth { request_id, auth } => f
                .debug_struct("RunAuth")
                .field("request_id", request_id)
                .field("auth", auth)
                .finish(),
            Self::Cancel { request_id } => f
                .debug_struct("Cancel")
                .field("request_id", request_id)
                .finish(),
            Self::CheckPort { port } => f.debug_struct("CheckPort").field("port", port).finish(),
            Self::SerialDebugOpen { cfg } => {
                f.debug_struct("SerialDebugOpen").field("cfg", cfg).finish()
            }
            Self::SerialDebugClose => f.write_str("SerialDebugClose"),
        }
    }
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
    // ── B7 local-transport hardening ─────────────────────────────────────────
    /// The user approved a dangerous operation on this connection. The token is
    /// the receipt of that one click, so the client can present it later instead
    /// of asking the user again.
    AuthGranted {
        token: String,
    },
    /// Every grant was revoked (托盘「撤销所有授权」), so the token the client
    /// stored is now worthless and this connection is unauthorized again.
    ///
    /// Pushed, hence no `request_id`. It exists so the client can drop its stored
    /// token immediately instead of discovering the revocation by wasting a flash
    /// attempt on it.
    AuthRevoked,
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

// ── Single active execution (B7) ─────────────────────────────────────────────

/// Why the process-wide execution right could not be taken.
enum ExecutionRefused {
    /// Another connection is driving a dangerous operation.
    OtherConnection,
    /// This connection's own earlier dangerous operation still holds it — most
    /// often a confirmation dialog the user has not answered yet.
    SameConnection,
}

/// Process-wide "one dangerous operation at a time" gate: at most one flash or
/// authorization is in flight anywhere in the helper at any instant, counting
/// the time its confirmation dialog spends waiting for the user. A conflicting
/// request is refused immediately with `execution_busy` — no queuing, same rule
/// as port arbitration (PRD: 占用即拒，不排队).
///
/// Two things this buys that the per-port [`PortArbiter`] cannot:
/// - **blast radius**: a second connection cannot drive a flash while another
///   one is running, not even on a *different* port (which the port table would
///   happily allow);
/// - **no dialog stacking**: while a confirmation is on screen, further
///   dangerous requests are refused instead of raising a second dialog on the
///   user.
///
/// Deliberately **not** re-entrant for the holder, and deliberately **not**
/// sticky per connection: the invariant is "one dangerous operation at a time,
/// whoever asks", not "the first connection owns the execution right for its
/// lifetime". Ownership by connection would let the earliest tab lock the helper
/// for as long as it stays open, which contradicts the product requirement that
/// several tabs may connect and watch (with only one of them driving) — so the
/// right is released the moment an operation finishes and the next asker gets it.
struct ExecutionArbiter {
    /// `Some(conn_id)` exactly while a dangerous operation is in flight.
    holder: Mutex<Option<u64>>,
}

impl ExecutionArbiter {
    fn new() -> Self {
        Self {
            holder: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<u64>> {
        self.holder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Take the execution right for `conn_id`.
    ///
    /// Released by dropping the returned guard — an RAII handle rather than a
    /// paired release call, so none of the many exit paths of a dangerous
    /// operation (rejection, timeout, entropy failure, `bad_request`,
    /// `port_busy`, a panicking job thread, a client disconnect) can leak it,
    /// and a future early return cannot forget to.
    fn try_acquire(self: &Arc<Self>, conn_id: u64) -> Result<ExecutionGuard, ExecutionRefused> {
        let mut holder = self.lock();
        match *holder {
            Some(owner) if owner == conn_id => Err(ExecutionRefused::SameConnection),
            Some(_) => Err(ExecutionRefused::OtherConnection),
            None => {
                *holder = Some(conn_id);
                Ok(ExecutionGuard {
                    arbiter: Arc::clone(self),
                })
            }
        }
    }
}

/// Holds the process-wide execution right until dropped.
struct ExecutionGuard {
    arbiter: Arc<ExecutionArbiter>,
}

impl Drop for ExecutionGuard {
    /// Clearing unconditionally is safe by construction: the arbiter hands out
    /// at most one guard at a time, so whoever holds this one is the holder.
    fn drop(&mut self) {
        *self.arbiter.lock() = None;
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
                serial_number: p.serial_number.clone(),
                usb_interface: p.usb_interface,
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

/// Map one core enumeration entry onto the bridge's port shape.
///
/// Split out of [`real_port_enumerator`] so the field mapping is testable
/// without real hardware: the closure below only exists to call
/// `list_serial_ports()`.
///
/// `serial_number` / `usb_interface` are forwarded **as reported**. The bridge
/// deliberately does not derive a device identity or a port role from them:
/// grouping ports into devices belongs to the client, which is also the only
/// side that knows the platform quirks (macOS reports the CDC data interfaces
/// 1 / 3 where Windows reports the control interfaces 0 / 2, and Linux often
/// reports none at all).
fn enumerated_from_core(entry: tyutool_core::SerialPortEntry) -> EnumeratedPort {
    EnumeratedPort {
        vid: entry.usb_vid,
        pid: entry.usb_pid,
        vendor: entry.usb_vid.and_then(vendor_for_vid),
        // Enumeration reports no ownership of its own; ports the bridge holds
        // for a job are folded in later by `apply_arbitration_busy`.
        busy: false,
        serial_number: entry.usb_serial,
        usb_interface: entry.usb_interface,
        path: entry.path,
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

        let ports: Vec<EnumeratedPort> = entries.into_iter().map(enumerated_from_core).collect();

        *last_good
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = ports.clone();
        ports
    })
}

// ── Production flash backend ─────────────────────────────────────────────────

/// Build the core job for one single-device authorization.
///
/// `FlashMode::Authorize` is dispatched by `registry::run_job` straight to
/// `authorize::run_authorize` — before any chip plugin is looked up, which is
/// also why a chip the registry does not know (`other`) authorizes fine. That
/// flow writes the *given* credentials to whatever device is on the port and
/// **never reads the device MAC**: the MAC is exclusively the batch pipeline's
/// lookup key into the Excel row that holds the credentials, and this flow has
/// no sheet to look anything up in.
///
/// Pure and separate from [`RealFlashBackend::run_auth`] so the shape of the
/// job — the part that decides which core flow runs — is testable without a
/// device.
fn authorize_job(spec: &AuthJobSpec) -> tyutool_core::FlashJob {
    tyutool_core::FlashJob {
        mode: tyutool_core::FlashMode::Authorize,
        chip_id: spec.chip_id.clone(),
        port: spec.port.clone(),
        baud_rate: spec.baud_rate,
        segments: None,
        flash_start_hex: None,
        flash_end_hex: None,
        erase_start_hex: None,
        erase_end_hex: None,
        read_start_hex: None,
        read_end_hex: None,
        read_file_path: None,
        firmware_path: None,
        authorize_uuid: Some(spec.uuid.clone()),
        authorize_key: Some(spec.auth_key.clone()),
        // `run_authorize` forces KV for single-device writes and ignores this
        // field; `None` keeps the bridge from ever looking like it asked for
        // the irreversible OTP burn (a batch-only feature).
        authorize_storage: None,
        // No callback = "the caller already confirmed": the dangerous-op gate
        // ran before this job started, so a device carrying other credentials
        // is overwritten rather than skipped (PRD 覆盖不可撤销) — the same
        // decision `ConflictPolicy::Overwrite` encoded on the batch path.
        confirm_overwrite: None,
    }
}

/// Translate one core `FlashEvent` into an authorization `progress` payload —
/// or `None` when nothing may be sent for it.
///
/// The wire contract for `run_auth` is `{"step": "<snake_case>"}` and nothing
/// else (PROTOCOL.md §progress), so this is a **narrowing** map, not a
/// pass-through: `run_job` forwards `FlashEvent` verbatim, `run_auth` does not.
///
/// It is also the credential firewall. `run_authorize` reports what it found on
/// the device through milestones, and two of them carry credentials in the
/// clear — `AuthReadComplete` (the device's uuid + authkey) and `AuthConflict`
/// (the credentials about to be overwritten). The GUI shows those in a secure
/// modal; on this transport they must simply not exist. The web client's own
/// filter is the *other* end being careful — this is the half we control.
///
/// Matched exhaustively on purpose: the next milestone someone adds to core
/// must become a compile error here, not a payload nobody classified.
fn auth_progress_payload(event: &tyutool_core::FlashEvent) -> Option<serde_json::Value> {
    use tyutool_core::FlashMilestone as M;

    let tyutool_core::FlashEvent::Milestone { milestone } = event else {
        // JobSummary / Phase / Percent / Warning / Done: none of them is a
        // `step`, and `Done` additionally carries the failure prose that the
        // bridge already answers with in its own `job_result` frame.
        return None;
    };

    match milestone {
        // The auth-write command has left the bridge — the one moment on this
        // path the user is waiting on, and a milestone that carries no
        // credential of its own.
        M::AuthWriteSent => Some(serde_json::json!({ "step": "writing_auth" })),

        // Credentials in the clear: never.
        M::AuthReadComplete { .. } | M::AuthConflict { .. } => None,

        // Real outcomes with no `step` equivalent — the terminal `job_result`
        // is what tells the client about them.
        M::AuthReadEmpty | M::AuthWriteSkipped => None,

        // Flash-side milestones; unreachable on the authorize path.
        M::HandshakeComplete
        | M::Connected { .. }
        | M::FlashIdRead { .. }
        | M::EraseComplete
        | M::SegmentWritten { .. }
        | M::WriteComplete
        | M::VerifyPassed
        | M::Rebooted => None,
    }
}

/// Run one single-device authorization through core and translate both halves
/// of its answer — the event stream and the failure — onto the wire.
///
/// `run_core` is injected for the same reason [`auth_after_natural_boot`]
/// injects its two steps: the real one is the only code in the bridge that can
/// reach a device, so the mapping it feeds has to be exercisable without one.
///
/// Sole guardian of two rules:
/// - **no credential leaves through either half** — the progress side is
///   narrowed by [`auth_progress_payload`], the failure side never formats a
///   uuid (core's own messages are credential-free, see `authorize.rs`);
/// - **`cancelled` and `cancelled_after_write` stay different facts** — the
///   second means the write command already reached the device, so the code may
///   be spent and the client is forbidden from saying 未写入 (PROTOCOL.md).
///   The batch slot used to draw that line for the bridge; on this path core
///   draws it with `FlashMilestone::AuthWriteSent`.
fn authorize_slot<R>(
    spec: &AuthJobSpec,
    progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    run_core: R,
) -> Result<(), JobError>
where
    R: FnOnce(
        &tyutool_core::FlashJob,
        &dyn Fn(tyutool_core::FlashEvent),
    ) -> Result<(), tyutool_core::FlashError>,
{
    let job = authorize_job(spec);
    let write_sent = AtomicBool::new(false);

    let on_event = |event: tyutool_core::FlashEvent| {
        if matches!(
            &event,
            tyutool_core::FlashEvent::Milestone {
                milestone: tyutool_core::FlashMilestone::AuthWriteSent
            }
        ) {
            write_sent.store(true, Ordering::Relaxed);
        }
        if let Some(payload) = auth_progress_payload(&event) {
            progress(payload);
        }
    };

    run_core(&job, &on_event).map_err(|e| {
        if matches!(e, tyutool_core::FlashError::Cancelled) && write_sent.load(Ordering::Relaxed) {
            return JobError {
                error_code: "cancelled_after_write".to_string(),
                // The port names the board in doubt. The batch path could name
                // its MAC; this one never reads one — and a uuid is not an
                // option, it is the very thing that must not travel.
                message: format!(
                    "cancelled after the authorization write command had already been sent \
                     to the device on {}; the authorization code may already be on it, \
                     so treat it as used",
                    spec.port
                ),
            };
        }
        JobError {
            error_code: auth_error_code(&e).to_string(),
            message: e.to_string(),
        }
    })
}

/// Map a core error raised on the authorization path onto its wire error code.
///
/// Pure and separate from [`RealFlashBackend::run_auth`] for the same reason as
/// [`authorize_slot`]: the code a client keys its copy off must be testable
/// without a device.
///
/// `device_no_response` is a distinct fact, not a flavour of `auth_failed`: the
/// device never said a word, so nothing was attempted on it and the honest
/// advice is "it may still be booting, retry" rather than "state unknown". The
/// classification is asked of core ([`tyutool_core::is_device_no_response`])
/// instead of matched on the message here — the bridge does not parse prose.
fn auth_error_code(err: &tyutool_core::FlashError) -> &'static str {
    match err {
        tyutool_core::FlashError::Cancelled => "cancelled",
        e if tyutool_core::is_device_no_response(e) => "device_no_response",
        _ => "auth_failed",
    }
}

/// Order the two device-touching steps of one authorization: let the device
/// finish booting on its own **first**, then hand the port to the auth slot.
///
/// Why this order is load-bearing: the slot's first act is a hardware reset
/// (`detect_firmware`). A reset fired into a device that is still running its
/// *first* boot after a firmware flash restarts that boot, so a client that
/// flashes and authorizes back to back — which is exactly what the web
/// workbench does — can restart the same boot indefinitely and never get a
/// byte back. The GUI's batch pipeline has always done the wait first
/// (`src-tauri/src/lib.rs`, "wait for the device to boot naturally after flash
/// before the auth slot issues a hardware reset"); the bridge did not, and
/// that difference is the whole bug.
///
/// The wait itself is **non-fatal by construction** (it cannot return an error:
/// a port it cannot open or a device that never talks simply falls through to
/// the slot, which resets and probes anyway), so this function only has to fix
/// the order.
///
/// Both steps are injected rather than called directly so the order is testable
/// without a board: the real caller is [`RealFlashBackend::run_auth`], which is
/// the only place that can reach a device.
fn auth_after_natural_boot<W, S>(
    spec: &AuthJobSpec,
    cancel: &AtomicBool,
    wait_for_natural_boot: W,
    run_slot: S,
) -> Result<(), JobError>
where
    W: FnOnce(&str, u32, &str, &AtomicBool),
    S: FnOnce() -> Result<(), JobError>,
{
    wait_for_natural_boot(&spec.port, spec.baud_rate, &spec.chip_id, cancel);
    run_slot()
}

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

    /// Maps the wire auth request onto tyutool-core's single-device
    /// authorization: `FlashMode::Authorize`, which `registry::run_job`
    /// dispatches to `authorize::run_authorize` before any chip plugin lookup.
    ///
    /// That is the CLI's `authorize` command — write *these* credentials to
    /// *this* device — and it is the flow CoBuilder actually asks for: the
    /// backend has already decided the uuid/authkey for the board on the port.
    ///
    /// It is emphatically **not** `run_batch_auth_slot`. The batch slot exists
    /// to bind a spreadsheet row to a device, so its first act is reading the
    /// MAC to look that row up — which is why an adapter with a `|_mac| None`
    /// lookup still died on `Failed to read MAC address` (T5AI field failure,
    /// 2026-07-31) on a board whose shell answered in 625 ms.
    ///
    /// Everything else about the request is preserved by [`authorize_job`]:
    /// KV storage, and an existing credential overwritten rather than skipped
    /// (PRD 覆盖不可撤销) because the dangerous-op gate already asked the user.
    fn run_auth(
        &self,
        spec: AuthJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        // Order fixed by `auth_after_natural_boot`: natural-boot wait, then the
        // authorization. A caller that flashes and authorizes back to back would
        // otherwise reset a device mid-first-boot and never get a byte back.
        auth_after_natural_boot(
            &spec,
            &cancel,
            tyutool_core::wait_after_firmware_flash,
            || {
                authorize_slot(&spec, progress, |job, on_event| {
                    tyutool_core::run_job(job, &cancel, on_event)
                })
            },
        )
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
    security: SecurityConfig,
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
        // Inert confirmation prompt, in-memory grants, audit into the log
        // channel: a host that wants a real dialog injects it below.
        security: SecurityConfig::default(),
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

    /// Wire the confirmation UI that gates `run_job` / `run_auth`.
    ///
    /// Without it every dangerous operation is refused (see [`DenyPrompt`]), so
    /// the production host must inject one; tests inject a fake that states what
    /// the user would have answered.
    pub fn with_auth_prompt(mut self, prompt: Arc<dyn AuthPrompt>) -> Self {
        self.security.prompt = prompt;
        self
    }

    /// Wire where issued grants are kept (default: in-memory, process-lifetime).
    pub fn with_token_store(mut self, tokens: Arc<dyn TokenStore>) -> Self {
        self.security.tokens = tokens;
        self
    }

    /// Wire where the confirmation audit trail goes (default: the developer log
    /// channel under the `bridge::audit` target).
    pub fn with_audit_sink(mut self, audit: Arc<dyn AuditSink>) -> Self {
        self.security.audit = audit;
        self
    }

    /// Shorten (or lengthen) the window an unanswered confirmation dialog gets
    /// before the operation is refused; [`CONFIRM_TIMEOUT`] by default.
    pub fn with_confirm_timeout(mut self, timeout: Duration) -> Self {
        self.security.confirm_timeout = timeout;
        self
    }

    /// Decide whether grants persisted by earlier sessions count on this server;
    /// [`GrantPolicy::Honour`] by default.
    ///
    /// [`GrantPolicy::Ignore`] is what an unattended host passes: combined with
    /// the refusing prompt it makes "no human present" mean "no dangerous
    /// operation", regardless of what an earlier attended session left in
    /// `grants.json`. See [`GrantPolicy`] for why that is the safe direction.
    pub fn with_grant_policy(mut self, policy: GrantPolicy) -> Self {
        self.security.grant_policy = policy;
        self
    }

    /// A handle for revoking every authorization ("撤销所有授权").
    ///
    /// Take it **before** `run_*`, which consumes the server — that is the whole
    /// reason it exists: the tray's UI thread needs the control while the server
    /// thread keeps serving.
    ///
    /// ⚠ It **snapshots the currently configured token store and audit sink**, so
    /// [`BridgeServer::with_token_store`] (and [`BridgeServer::with_audit_sink`])
    /// must be called first — an `Authority` taken before them would keep
    /// revoking the store it saw, i.e. the default in-memory one, and leave the
    /// grant file on disk untouched.
    pub fn authority(&self) -> Authority {
        Authority {
            tokens: Arc::clone(&self.security.tokens),
            audit: Arc::clone(&self.security.audit),
            connections: Arc::clone(&self.security.connections),
        }
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
        // Ports are exclusive one by one; dangerous operations are exclusive as a
        // whole (see [`ExecutionArbiter`]), so the two tables sit side by side.
        let execution = Arc::new(ExecutionArbiter::new());
        let stats = Arc::new(StatsPublisher::new(stats_tx));
        let security = Arc::new(self.security);

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
                        Arc::clone(&execution),
                        Arc::clone(&backend),
                        Arc::clone(&stats),
                        Arc::clone(&security),
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
    /// The allowlisted `Origin` this connection completed its handshake with;
    /// shown to the user in the confirmation dialog and recorded in the audit
    /// trail.
    origin: String,
    /// Did a human approve a dangerous operation *in this session*, through the
    /// confirmation dialog?
    ///
    /// Starts `false` on every connection and is flipped by the first approval,
    /// which also issues the token that lets the next connection skip the
    /// dialog. Cleared by [`Authority::revoke_all`]. This flag covers the
    /// dialog half of authorization only — the token half is re-derived from the
    /// store, see [`ConnContext::is_authorized`].
    approved: AtomicBool,
    /// The decoded `?token=` this connection presented at handshake time, if
    /// any. Kept (rather than reduced to a bool once) so the gate can re-ask the
    /// store whether it is *still* granted.
    presented_token: Option<String>,
    security: Arc<SecurityConfig>,
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
    /// The process-wide "one dangerous operation at a time" gate, shared with
    /// every other connection.
    execution: Arc<ExecutionArbiter>,
    backend: Arc<dyn FlashBackend>,
    ports: Arc<PortsBroadcaster>,
    /// Jobs started by this connection and not yet finished, so a disconnect
    /// does not leave a serial port held forever.
    inflight: Mutex<HashSet<String>>,
    /// Requests of this connection whose confirmation is still pending, each
    /// with the one-shot that ends the wait.
    ///
    /// A job waiting for consent has claimed no port and is not in `inflight`
    /// yet, so neither [`PortArbiter::cancel`] nor the disconnect teardown can
    /// reach it — this registry is what makes a `cancel` frame (and a closed
    /// tab) able to end an operation *during* the confirmation window, instead
    /// of the client showing 「已取消」 while a click seconds later still writes
    /// the device.
    ///
    /// Keyed by `request_id` alone: the map is per connection, which is the same
    /// namespace `by_request` uses, so a `cancel` still cannot cross connection
    /// boundaries.
    pending_confirms: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    /// This connection's serial monitor, at most one at a time.
    session: Mutex<SessionSlot>,
}

/// Keeps one pending confirmation listed for as long as the wait lasts.
///
/// RAII rather than a paired remove call, for the same reason as
/// [`ExecutionGuard`]: the confirmation wait has many exits (approval, refusal,
/// timeout, an abandoned responder, a cancel, a disconnect) and a future early
/// return must not be able to leave a stale sender behind — a later `cancel`
/// would then be answered as if a dialog were still up.
struct PendingConfirm {
    ctx: Arc<ConnContext>,
    request_id: String,
}

impl Drop for PendingConfirm {
    fn drop(&mut self) {
        self.ctx.lock_pending_confirms().remove(&self.request_id);
    }
}

impl ConnContext {
    /// May this connection run dangerous operations without asking the user?
    ///
    /// The one place that decides. Two independent sources of privilege:
    /// a confirmation the user clicked on *this* connection (`approved`), or a
    /// `?token=` presented at handshake time that the store **still** grants for
    /// this exact Origin.
    ///
    /// The token half is a live lookup on purpose, not a bool cached at
    /// handshake time. Caching it would make privilege outlive the grant it came
    /// from: any revocation that does not walk the live-connection registry — one
    /// landing in the same instant a connection is being registered, or a future
    /// revocation path that simply forgets to walk it — would leave an already
    /// open tab fully privileged after the user revoked. Re-reading the store
    /// makes "the grant is gone" and "the connection is unprivileged" the same
    /// fact instead of two facts someone has to keep in sync.
    ///
    /// The cost is one lookup per dangerous operation instead of one per
    /// connection: a mutex lock plus a short `Vec` scan over the in-memory grant
    /// list (`FileTokenStore` caches it and never reads the disk here). That is
    /// nothing next to a flash job, so please do not "optimize" it back into a
    /// cached bool.
    ///
    /// Under [`GrantPolicy::Ignore`] the token half is skipped entirely: the
    /// store is shared with the attended tray shell, so consulting it here is
    /// exactly how an unattended run would inherit a confirmation a human gave
    /// at a keyboard days ago.
    ///
    /// The `approved` half staying outside the policy is a deliberate
    /// library-level invariant, not an oversight: a dialog somebody actually
    /// answered *on this connection* and a stale line in a shared file are
    /// different kinds of evidence, and only the second one is what
    /// [`GrantPolicy::Ignore`] exists to distrust. Under the current wiring that
    /// branch is unreachable — `Ignore` is only ever paired with a prompt that
    /// refuses everything — so it is intentionally untested. Should a future host
    /// combine `Ignore` with a prompt that *can* approve (say a headless mode
    /// that asks over a control channel), this branch starts carrying traffic,
    /// and honouring that fresh answer is the intended behaviour.
    fn is_authorized(&self) -> bool {
        self.approved.load(std::sync::atomic::Ordering::Relaxed)
            || (self.security.grant_policy == GrantPolicy::Honour
                && self
                    .presented_token
                    .as_deref()
                    .is_some_and(|token| self.security.tokens.is_granted(token, &self.origin)))
    }

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

    fn lock_pending_confirms(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, tokio::sync::oneshot::Sender<()>>> {
        self.pending_confirms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// List `request_id` as waiting for its confirmation and hand back the
    /// receiving end of its cancel signal.
    ///
    /// Must be called *before* the prompt is raised, so a `cancel` arriving in
    /// the same instant as the dialog cannot fall between the two.
    fn begin_pending_confirm(
        self: &Arc<Self>,
        request_id: &str,
    ) -> (PendingConfirm, tokio::sync::oneshot::Receiver<()>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // No key collision is possible: the execution right is taken before the
        // prompt and held for the whole job, so this connection cannot have a
        // second dangerous operation (let alone the same `request_id`) pending.
        self.lock_pending_confirms()
            .insert(request_id.to_string(), tx);
        (
            PendingConfirm {
                ctx: Arc::clone(self),
                request_id: request_id.to_string(),
            },
            rx,
        )
    }

    /// End the confirmation wait of `request_id`; `false` means this connection
    /// has no confirmation pending under that id (already answered, already
    /// running, or never asked) and the caller should look elsewhere.
    fn cancel_pending_confirm(&self, request_id: &str) -> bool {
        match self.lock_pending_confirms().remove(request_id) {
            // Err only if the waiter is already gone, which makes the signal
            // moot — the operation ended on its own.
            Some(tx) => tx.send(()).is_ok(),
            None => false,
        }
    }

    /// End every confirmation wait of this connection (it is going away), and
    /// report how many there were.
    fn cancel_all_pending_confirms(&self) -> usize {
        let pending: Vec<tokio::sync::oneshot::Sender<()>> = self
            .lock_pending_confirms()
            .drain()
            .map(|(_, tx)| tx)
            .collect();
        let count = pending.len();
        for tx in pending {
            let _ = tx.send(());
        }
        count
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

/// What the handshake callback learned about a connection it accepted, handed
/// out through a slot because the callback is the only place that sees the
/// request.
struct AcceptedHandshake {
    /// The matched allowlist entry (the confirmation dialog has to name who is
    /// asking).
    origin: &'static str,
    /// The decoded `?token=` the client presented, `None` when it presented
    /// none. Carried into the connection because the gate re-checks it against
    /// the store on every dangerous operation — see
    /// [`ConnContext::is_authorized`].
    presented_token: Option<String>,
    /// Was `presented_token` recognized for this exact Origin *at handshake
    /// time*, under the configured [`GrantPolicy`]? Only drives the log line,
    /// the `connect` audit line and "do not pop a pointless first dialog"; it is
    /// not itself the authorization.
    pre_authorized: bool,
    /// Fingerprint of the presented token for the audit line; `None` when the
    /// client presented none.
    token_fingerprint: Option<String>,
}

// The handshake callback below returns tungstenite's `ErrorResponse` by value:
// its signature is fixed by the `accept_hdr_async` contract, so the large `Err`
// variant cannot be boxed away (same reason as on [`allowlisted_origin`]).
#[allow(clippy::result_large_err)]
async fn handle_connection(
    stream: tokio::net::TcpStream,
    ports: Arc<PortsBroadcaster>,
    arbiter: Arc<PortArbiter>,
    execution: Arc<ExecutionArbiter>,
    backend: Arc<dyn FlashBackend>,
    stats: Arc<StatsPublisher>,
    security: Arc<SecurityConfig>,
) {
    // The handshake callback is the only place that sees the request, so it
    // hands out both the matched Origin and the `?token=` verdict through this
    // slot for the connection to carry.
    let accepted: Arc<Mutex<Option<AcceptedHandshake>>> = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&accepted);
    let handshake_security = Arc::clone(&security);
    let ws = match tokio_tungstenite::accept_hdr_async(
        stream,
        move |req: &Request, response: Response| {
            let origin = allowlisted_origin(req)?;
            // A token problem never refuses the connection — it only downgrades
            // it, so a stale grant from a previous install cannot lock the user
            // out of the device list. 403 stays exclusively the Origin check's.
            let presented = token_from_query(req.uri().query());
            // The policy applies here too, not just at the gate: under
            // `GrantPolicy::Ignore` a stored grant buys nothing, so a log line or
            // an audit line claiming `pre_authorized=true` would simply be false.
            let pre_authorized = handshake_security.grant_policy == GrantPolicy::Honour
                && presented
                    .as_deref()
                    .is_some_and(|token| handshake_security.tokens.is_granted(token, origin));
            let token_fingerprint = presented.as_deref().map(redact_token);
            match (&token_fingerprint, pre_authorized) {
                (Some(fingerprint), true) => log::info!(
                    "bridge connection pre-authorized (origin={origin}, token={fingerprint})"
                ),
                (Some(fingerprint), false) => log::info!(
                    "bridge token not recognized, connection continues unauthorized \
                     (origin={origin}, token={fingerprint})"
                ),
                (None, _) => {}
            }
            *captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(AcceptedHandshake {
                origin,
                presented_token: presented,
                pre_authorized,
                token_fingerprint,
            });
            Ok(response)
        },
    )
    .await
    {
        Ok(ws) => ws,
        Err(e) => {
            // Covers both a rejected Origin and a malformed handshake.
            log::warn!("bridge WS handshake not completed: {e}");
            return;
        }
    };
    let accepted = accepted
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .unwrap_or_else(|| {
            // Unreachable: a completed handshake ran the callback above. Fall
            // back to an Origin no allowlist entry can equal, and to no
            // authorization at all, rather than panic in a resident process.
            log::error!("bridge accepted a connection whose handshake was not recorded");
            AcceptedHandshake {
                origin: "",
                presented_token: None,
                pre_authorized: false,
                token_fingerprint: None,
            }
        });
    let origin = accepted.origin;
    security.audit.record(&format!(
        "connect origin={origin} pre_authorized={} token={}",
        accepted.pre_authorized,
        accepted.token_fingerprint.as_deref().unwrap_or("-")
    ));
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

    let connections = Arc::clone(&security.connections);
    let ctx = Arc::new(ConnContext {
        conn_id: arbiter.next_conn_id(),
        origin: origin.to_string(),
        approved: AtomicBool::new(false),
        presented_token: accepted.presented_token,
        security,
        sink_tx: Mutex::new(Some(sink_tx)),
        shutdown: tokio::sync::Notify::new(),
        arbiter,
        execution,
        backend,
        ports,
        inflight: Mutex::new(HashSet::new()),
        pending_confirms: Mutex::new(HashMap::new()),
        session: Mutex::new(SessionSlot::Idle),
    });

    // Listed from here on, so a revocation can clear an in-session approval and
    // push `auth_revoked` to this connection. Dropped (deregistered) on every
    // exit below.
    let _registered = connections.register(&ctx);

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

    // A request still waiting for its confirmation is *not* in `inflight` (it has
    // claimed no port), yet it holds the process-wide execution right for the
    // whole confirmation window. Without this, one abandoned tab would lock every
    // other tab out of flashing for up to 60s.
    let pending = ctx.cancel_all_pending_confirms();
    if pending > 0 {
        log::warn!(
            "bridge connection closed with {pending} confirmation(s) pending, cancelling them"
        );
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
/// A frame that fails to decode is *answered*, not dropped: see
/// [`reject_unparsable`].
fn dispatch(ctx: &Arc<ConnContext>, text: &str) {
    let message: ClientMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(e) => {
            reject_unparsable(ctx, text, &e);
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
            // Two places a cancellable request can be: still waiting for the
            // user's confirmation (no port claimed, so the port arbiter has
            // never heard of it), or already running. The registry is checked
            // first because a request can only be in one of the two.
            if ctx.cancel_pending_confirm(&request_id) {
                log::info!(
                    "bridge cancel ended job {request_id} while its confirmation was pending"
                );
            } else if !ctx.arbiter.cancel(ctx.conn_id, &request_id) {
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

/// Answer a frame the strict decoder rejected.
///
/// Dropping it (what the bridge used to do) is invisible to the client: it sent
/// a `run_job` and simply never heard back, so the page sat on "等待确认…0%"
/// until the user gave up. A client must be able to fail fast, so an undecodable
/// request is answered with the existing `bad_request` code.
///
/// Correlation is best-effort: the strict decode failed, but `type` and
/// `request_id` are plain top-level strings and usually survive whatever broke
/// the payload, so they are re-read from a lenient `Value` parse.
///
/// - `serial_debug_open` answers `serial_debug_open_failed` (that frame class
///   carries no `request_id` — a connection has at most one session);
/// - anything else carrying a non-empty string `request_id` answers
///   `job_result` — including an unknown `type`, so a client on a newer protocol
///   fails fast instead of waiting;
/// - a frame with neither (unknown `type` and no usable `request_id`, or text
///   that is not JSON at all) is logged and dropped: there is nothing to address
///   an answer to, and inventing a `request_id` would answer a task the client
///   never started.
fn reject_unparsable(ctx: &Arc<ConnContext>, text: &str, error: &serde_json::Error) {
    let started = Instant::now();
    let reason = parse_failure_reason(error);
    // Deliberately logged without the frame itself: it may carry a base64
    // firmware image, a device uuid or an auth key.
    log::warn!("bridge rejecting unparsable client frame: {reason}");

    let envelope: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let string_field = |name: &str| {
        envelope
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(serde_json::Value::as_str)
            .filter(|found| !found.is_empty())
            .map(str::to_string)
    };

    if string_field("type").as_deref() == Some("serial_debug_open") {
        ctx.send(&open_failed("bad_request", reason));
    } else if let Some(request_id) = string_field("request_id") {
        ctx.send(&failed(&request_id, started, "bad_request", reason));
    } else {
        log::warn!(
            "bridge dropped the unparsable frame: no request_id to answer it with \
             (the client will have to time out on its own)"
        );
    }
}

/// Render a decode failure as structure only — never the data that caused it.
///
/// `serde_json::Error`'s own `Display` embeds the offending value for a type
/// mismatch (`invalid type: string "…"`) and the offending tag for an unknown
/// variant, and this text travels to the client and into the log file. Client
/// frames carry base64 firmware, device uuids and auth keys, so the value is
/// dropped and only the field name (ours, from the struct definition), the
/// failure class and the position survive.
fn parse_failure_reason(error: &serde_json::Error) -> String {
    let rendered = error.to_string();
    // The one shape worth forwarding verbatim: the name comes from our own
    // `#[derive(Deserialize)]` structs, never from the frame.
    if let Some(field) = rendered
        .strip_prefix("missing field `")
        .and_then(|rest| rest.split('`').next())
    {
        return format!("missing required field `{field}`");
    }
    if rendered.starts_with("unknown variant") {
        return "unknown or unsupported frame `type`".to_string();
    }
    let (line, column) = (error.line(), error.column());
    match error.classify() {
        serde_json::error::Category::Data => {
            format!("wrong type for a field at line {line} column {column}")
        }
        serde_json::error::Category::Syntax => {
            format!("malformed JSON at line {line} column {column}")
        }
        serde_json::error::Category::Eof => "truncated JSON frame".to_string(),
        serde_json::error::Category::Io => "frame could not be read".to_string(),
    }
}

/// Everything a dangerous operation has to clear before it may touch a device:
/// the process-wide execution right first, the human confirmation second.
///
/// `None` back means the operation is off and the `job_result` frame explaining
/// why has already been sent. `Some(guard)` must be held for the whole job: the
/// execution right is released by dropping it, i.e. after the terminal
/// `job_result`, not merely after the confirmation — a confirmed job is still an
/// operation nobody else may start alongside.
///
/// The exclusion is taken *before* the prompt on purpose: that is what keeps a
/// retrying client (or a rogue local process hammering the port) from stacking a
/// second dialog on the user while the first one is still on screen.
async fn admit_dangerous_op(
    ctx: &Arc<ConnContext>,
    request_id: &str,
    started: Instant,
    request: ConfirmRequest,
) -> Option<ExecutionGuard> {
    let execution = match ctx.execution.try_acquire(ctx.conn_id) {
        Ok(guard) => guard,
        Err(refused) => {
            // One machine-readable code for both cases; the message is what tells
            // a developer reading the logs which of the two they are looking at.
            let message = match refused {
                ExecutionRefused::OtherConnection => {
                    "another connection is already running a dangerous operation; \
                     the bridge runs one at a time"
                }
                ExecutionRefused::SameConnection => {
                    "this connection already has a dangerous operation in flight \
                     (waiting for the user's confirmation, or running)"
                }
            };
            audit_confirm(ctx.security.audit.as_ref(), &request, "execution_busy");
            ctx.send(&failed(
                request_id,
                started,
                "execution_busy",
                message.to_string(),
            ));
            return None;
        }
    };

    if authorize_dangerous_op(ctx, request_id, started, request).await {
        Some(execution)
    } else {
        None
    }
}

/// The human-in-the-loop gate every dangerous operation passes (B7).
///
/// Runs before the port is claimed, before the firmware is decoded and before
/// any backend call, so a refusal costs the device nothing and leaves no claim
/// behind. `false` back means the operation is off and the `job_result` frame
/// explaining why has already been sent.
///
/// Consent is per connection: the first approval flips `ctx.approved`, so a
/// client that flashes ten boards in a row is asked once — and a connection whose
/// `?token=` the store still grants for its Origin is never asked at all (and is
/// issued no second token). Both halves are decided by
/// [`ConnContext::is_authorized`], which re-reads the store rather than trusting
/// a verdict cached at handshake time.
async fn authorize_dangerous_op(
    ctx: &Arc<ConnContext>,
    request_id: &str,
    started: Instant,
    request: ConfirmRequest,
) -> bool {
    if ctx.is_authorized() {
        // No dialog, but still one audit line: without it a session's second and
        // later flashes would be invisible to a review, which is exactly the
        // history an incident needs.
        audit_confirm(ctx.security.audit.as_ref(), &request, "preauthorized");
        return true;
    }

    let security = Arc::clone(&ctx.security);
    // Listed before the prompt is raised, so a `cancel` frame that arrives in the
    // same instant as the dialog cannot fall between registration and display.
    // `_pending` deregisters on every exit path below.
    let (_pending, cancel_rx) = ctx.begin_pending_confirm(request_id);
    // One-shot rather than a callback into the connection: `respond` may be
    // invoked from any thread (a native dialog answers on the UI thread), and a
    // oneshot sender is both `Send` and non-async to fire.
    let (answer_tx, answer_rx) = tokio::sync::oneshot::channel::<ConfirmDecision>();
    security.prompt.request(
        request.clone(),
        Box::new(move |decision| {
            // Err only means this task already gave up (timeout or a cancel);
            // the decision is then moot.
            let _ = answer_tx.send(decision);
        }),
    );

    // Three ways out: the user answered, the client took the request back, or
    // nobody answered in time.
    //
    // Accepted limitation: a `cancel` (or a disconnect) ends the *wait*, not the
    // dialog. `AuthPrompt` has no dismiss operation, so an OS dialog already on
    // screen lingers until the user dismisses it or the platform gives up, and
    // its late answer lands in the dropped `answer_tx` and is ignored — the same
    // trade-off already documented on the Windows MessageBox arm. What matters
    // is that a late 「允许」 can no longer start the operation.
    //
    // `biased` with the cancel branch first so a cancellation that landed in the
    // same instant as the answer always wins: not running a dangerous operation
    // is the safe direction, and an unbiased `select!` would decide that race by
    // coin flip.
    let outcome = tokio::select! {
        biased;
        // `Err` means the sender vanished without signalling, which no path does
        // today; treated as a cancellation anyway, same fail-closed reasoning.
        _ = cancel_rx => "cancelled",
        answered = tokio::time::timeout(security.confirm_timeout, answer_rx) => match answered {
            Ok(Ok(ConfirmDecision::Approve)) => "approved",
            Ok(Ok(ConfirmDecision::Reject)) => "rejected",
            // The responder was dropped without being called: no consent was
            // given, which is a refusal — silence never opens the door.
            Ok(Err(_)) => "abandoned",
            Err(_) => "timeout",
        },
    };
    audit_confirm(security.audit.as_ref(), &request, outcome);

    if outcome == "cancelled" {
        // Deliberately the plain `cancelled` code, not one of its own: nothing
        // reached the device, and the client already renders `cancelled` as
        // 「已取消」 — which is exactly what the user asked for and what the
        // device state is.
        ctx.send(&failed(
            request_id,
            started,
            "cancelled",
            format!(
                "the client cancelled this {} while its confirmation was still pending",
                request.op.as_str()
            ),
        ));
        return false;
    }

    if outcome != "approved" {
        ctx.send(&failed(
            request_id,
            started,
            "user_rejected",
            format!(
                "the user did not confirm this {} ({outcome})",
                request.op.as_str()
            ),
        ));
        return false;
    }

    // Persist the click as a token so the user is not asked again.
    let Some(token) = new_token() else {
        ctx.send(&failed(
            request_id,
            started,
            "internal",
            "cannot issue a confirmation token, no system entropy available".to_string(),
        ));
        return false;
    };
    security.tokens.insert(Grant {
        token: token.clone(),
        origin: request.origin.clone(),
        granted_at_ms: now_ms(),
    });
    ctx.approved
        .store(true, std::sync::atomic::Ordering::Relaxed);
    security.audit.record(&format!(
        "grant origin={} op={} token={}",
        request.origin,
        request.op.as_str(),
        redact_token(&token)
    ));
    ctx.send(&ServerFrame::AuthGranted { token });
    true
}

/// One flash job end to end: take the execution right, confirm with the user,
/// claim the port, then decode and flash off the async worker (base64 decoding a
/// multi-MB image is CPU work that belongs on the blocking pool, like every other
/// synchronous step here).
async fn run_job_task(
    ctx: Arc<ConnContext>,
    request_id: String,
    job: WireJob,
    file_content: String,
) {
    let started = Instant::now();

    // Before the claim and before the decode: the size shown to the user is
    // derived from the base64 text, so a refused job never pays for a multi-MB
    // decode and never holds the port. `_execution` keeps the process-wide
    // execution right for the rest of this task.
    let Some(_execution) = admit_dangerous_op(
        &ctx,
        &request_id,
        started,
        ConfirmRequest {
            op: DangerousOp::Flash,
            origin: ctx.origin.clone(),
            chip_id: job.chip_id.clone(),
            port: job.port.clone(),
            firmware_bytes: base64_decoded_len(&file_content),
        },
    )
    .await
    else {
        return;
    };

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
/// malformed request neither claims the port nor reaches the device — and, since
/// validation precedes the confirmation gate, does not bother the user with a
/// dialog for a request that cannot run anyway.
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

    // Length is the same kind of fact as emptiness — the firmware's own
    // constraint on the pair (`tuya_authorize.c`), knowable without a device —
    // so it is answered with the same `bad_request` rather than `auth_failed`:
    // nothing was attempted on the board, and a client must not tell its user
    // the device may have been touched. `run_authorize` (the CLI's authorize
    // path, which this bridge now uses) does *not* check, so an over-long uuid
    // would otherwise be written to the device and only fail at verify, having
    // spent a real authorization code. The rule itself is core's — one copy.
    if let Err(e) = tyutool_core::validate_auth_credentials(auth.uuid.trim(), auth.auth_key.trim())
    {
        // Core's message carries the offending *length*, never the credential.
        ctx.send(&failed(&request_id, started, "bad_request", e.to_string()));
        return;
    }

    // Overwriting an authorization code cannot be undone (PRD: 覆盖不可撤销), so
    // it is gated exactly like flashing. The dialog names no credential: `uuid`
    // and `auth_key` never leave the job path.
    let Some(_execution) = admit_dangerous_op(
        &ctx,
        &request_id,
        started,
        ConfirmRequest {
            op: DangerousOp::Authorize,
            origin: ctx.origin.clone(),
            chip_id: auth.chip_id.clone(),
            port: auth.port.clone(),
            firmware_bytes: None,
        },
    )
    .await
    else {
        return;
    };

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
/// [`ORIGIN_ALLOWLIST`]; the matched entry is handed back so the accepted
/// connection can carry it.
///
/// A non-allowlisted Origin is refused with 403 and the socket is dropped; a
/// missing Origin is treated as a non-browser caller and refused the same way
/// (非白名单直接断开，缺失 Origin 视为非浏览器来源同样拒绝).
///
/// This is a filter, not a trust anchor: `Origin` is a header the browser is
/// forced to add, not one a native local process has to tell the truth about.
/// Dangerous operations therefore additionally pass [`admit_dangerous_op`].
// The Result signature is fixed by tungstenite's handshake callback contract,
// so the large `ErrorResponse` variant cannot be boxed away here.
#[allow(clippy::result_large_err)]
fn allowlisted_origin(req: &Request) -> Result<&'static str, ErrorResponse> {
    let origin = req.headers().get("Origin");
    let matched = origin.and_then(|value| {
        ORIGIN_ALLOWLIST
            .iter()
            .find(|allowed| value.as_bytes() == allowed.as_bytes())
            .copied()
    });
    if let Some(matched) = matched {
        return Ok(matched);
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
    fn production_enumeration_carries_serial_number_and_usb_interface() {
        // Real values from a T5 board (`tyutool-cli usb-port-survey`): both UART
        // bridges of one physical device share a serial number and differ only
        // by USB interface. Dropping either field is what made the web UI count
        // one board as two devices.
        let entry = tyutool_core::SerialPortEntry {
            path: "/dev/cu.usbmodem56D70427243".to_string(),
            name: Some("USB Dual_Serial".to_string()),
            usb_vid: Some(0x1A86),
            usb_pid: Some(0x55D2),
            usb_serial: Some("56D7042724".to_string()),
            usb_interface: Some(3),
            port_role: None,
        };

        let port = enumerated_from_core(entry);

        assert_eq!(port.path, "/dev/cu.usbmodem56D70427243");
        assert_eq!(port.serial_number.as_deref(), Some("56D7042724"));
        assert_eq!(port.usb_interface, Some(3));
        assert_eq!(port.vendor.as_deref(), Some("WCH"));
    }

    #[test]
    fn production_enumeration_omits_both_fields_for_a_non_usb_port() {
        let entry = tyutool_core::SerialPortEntry {
            path: "/dev/cu.Bluetooth-Incoming-Port".to_string(),
            name: None,
            usb_vid: None,
            usb_pid: None,
            usb_serial: None,
            usb_interface: None,
            port_role: None,
        };

        let port = enumerated_from_core(entry);

        assert_eq!(port.serial_number, None);
        assert_eq!(port.usb_interface, None);
    }

    #[test]
    fn device_count_ignores_ports_without_an_allowlisted_vid() {
        let port = |vid: Option<u16>| EnumeratedPort {
            path: format!("/dev/tty.{vid:?}"),
            vid,
            pid: None,
            vendor: None,
            busy: false,
            serial_number: None,
            usb_interface: None,
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

    #[test]
    fn decoded_length_is_derived_from_the_base64_text_alone() {
        // Expected values produced independently (`python3 -c "import base64;
        // len(base64.b64decode(s))"`), not by calling the decoder under test.
        assert_eq!(base64_decoded_len(""), Some(0));
        assert_eq!(base64_decoded_len("aGVsbG8="), Some(5)); // "hello"
        assert_eq!(base64_decoded_len("aGk="), Some(2)); // "hi"
        assert_eq!(base64_decoded_len("YWJj"), Some(3)); // "abc", unpadded
        assert_eq!(base64_decoded_len("YWJjZA=="), Some(4)); // "abcd"
    }

    #[test]
    fn malformed_base64_has_no_derivable_length() {
        // Not a whole number of quanta.
        assert_eq!(base64_decoded_len("aGVsbG8"), None);
        // Three pad characters: no quantum decodes to zero bytes.
        assert_eq!(base64_decoded_len("a==="), None);
        // Padding in the middle rather than at the end.
        assert_eq!(base64_decoded_len("aGk=aGk="), None);
    }

    #[test]
    fn a_redacted_token_reveals_neither_the_secret_nor_more_than_its_length() {
        let token = "Ab3-xYz_0123456789";

        let redacted = redact_token(token);

        assert_eq!(redacted, "Ab3-xY…(len=18)");
        assert!(
            !redacted.contains("z_0123456789"),
            "the tail must not survive redaction: {redacted}"
        );
    }

    /// Shorthand: what the handshake would compare against the store.
    fn token_of(query: &str) -> Option<String> {
        token_from_query(Some(query))
    }

    #[test]
    fn a_token_query_value_is_percent_decoded() {
        // Escape sequences taken from RFC 3986 §2.1 (`%20` = space, `%2B` = '+'),
        // not from this decoder.
        assert_eq!(token_of("token=Ab3-xYz_09"), Some("Ab3-xYz_09".to_string()));
        assert_eq!(token_of("token=tok%20en"), Some("tok en".to_string()));
        assert_eq!(token_of("token=tok%2Ben"), Some("tok+en".to_string()));
        // Hex is case-insensitive.
        assert_eq!(token_of("token=tok%2ben"), Some("tok+en".to_string()));
        // Multi-byte UTF-8 (「你」 = E4 BD A0), reassembled across three escapes.
        assert_eq!(token_of("token=%E4%BD%A0"), Some("你".to_string()));
    }

    #[test]
    fn a_plus_in_a_token_query_value_is_a_literal_plus() {
        // This is a URI query value, not a form body: `+` is not a space.
        assert_eq!(token_of("token=tok+en"), Some("tok+en".to_string()));
    }

    #[test]
    fn a_malformed_escape_makes_the_token_unusable() {
        // Unusable rather than "compared raw": the connection then downgrades to
        // unauthorized, which costs one confirmation instead of matching text the
        // client never sent.
        assert_eq!(token_of("token=%"), None);
        assert_eq!(token_of("token=tok%"), None);
        assert_eq!(token_of("token=tok%A"), None);
        assert_eq!(token_of("token=tok%ZZ"), None);
        assert_eq!(token_of("token=tok%2"), None);
        // Valid escapes, but not a UTF-8 sequence (a lone continuation byte, and
        // an unfinished 3-byte sequence): must be rejected, never panic.
        assert_eq!(token_of("token=%A0"), None);
        assert_eq!(token_of("token=%E4%BD"), None);
        assert_eq!(token_of("token=%FF%FE"), None);
    }

    #[test]
    fn an_absent_or_empty_token_parameter_counts_as_no_token() {
        assert_eq!(token_from_query(None), None);
        assert_eq!(token_of(""), None);
        assert_eq!(token_of("token="), None);
        assert_eq!(token_of("a=1&b=2"), None);
        // Only a full `token=` key matches, not a suffix of another key.
        assert_eq!(token_of("mytoken=abc"), None);
    }

    #[test]
    fn the_token_parameter_is_found_among_others_and_the_first_one_wins() {
        assert_eq!(token_of("a=1&token=X&b=2"), Some("X".to_string()));
        assert_eq!(
            token_of("token=first&token=second"),
            Some("first".to_string())
        );
    }
    /// A device that never answered is not "the authorization failed": nothing
    /// was attempted on it. The web client must be able to say "it is probably
    /// still booting, retry" *structurally*, the same way it tells the two
    /// cancellations apart — by code, never by parsing the message.
    #[test]
    fn a_device_that_never_answered_gets_its_own_code() {
        let silent = tyutool_core::FlashError::Plugin(format!(
            "{} within 30.0 s after reset — ...",
            tyutool_core::DEVICE_NO_RESPONSE_PREFIX
        ));
        assert_eq!(auth_error_code(&silent), "device_no_response");

        // Everything else on the auth path keeps its existing code.
        assert_eq!(
            auth_error_code(&tyutool_core::FlashError::Plugin(
                "Verification failed: no response from auth-read".to_string()
            )),
            "auth_failed"
        );
        assert_eq!(
            auth_error_code(&tyutool_core::FlashError::Cancelled),
            "cancelled"
        );
    }

    /// Field failure this encodes (T5AI, 2026-07-31): the web workbench flashes
    /// and authorizes back to back, and the authorization slot's very first act
    /// is a hardware reset. Firing that reset into a device that is still
    /// running its *first* boot after a flash restarts that boot, so it never
    /// completes — three consecutive runs read zero bytes for the whole 30 s
    /// probe window, while the same board answered in 627 / 636 / 642 ms once it
    /// had been left alone long enough to finish booting.
    ///
    /// The fix is the one the GUI's batch pipeline already documents: let the
    /// device boot naturally *before* handing the port to the slot. So the order
    /// of the two device-touching steps is the behaviour under test.
    #[test]
    fn an_authorization_lets_the_device_boot_before_the_slot_resets_it() {
        let steps = std::cell::RefCell::new(Vec::<String>::new());
        let spec = AuthJobSpec {
            chip_id: "T5AI".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            baud_rate: 921_600,
            uuid: "uuidxxxxxxxxxxxxxxxx".to_string(),
            auth_key: "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
        };
        let cancel = AtomicBool::new(false);

        let outcome = auth_after_natural_boot(
            &spec,
            &cancel,
            |port, baud_rate, chip_id, _cancel| {
                steps
                    .borrow_mut()
                    .push(format!("wait {port} {baud_rate} {chip_id}"));
            },
            || {
                steps.borrow_mut().push("slot".to_string());
                Ok(())
            },
        );

        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            steps.into_inner(),
            vec![
                // Same arguments the GUI passes: the port, the *auth* baud rate
                // and the chip id, so the wait speaks the device's protocol.
                "wait /dev/tty.fakeA 921600 T5AI".to_string(),
                "slot".to_string(),
            ],
            "the natural-boot wait must precede the slot's hardware reset"
        );
    }

    /// Field failure this encodes (T5AI, 2026-07-31, fifth round): with the
    /// natural-boot wait in place the shell answered in 625 ms and the firmware
    /// was detected — and the authorization *still* died on `Failed to read MAC
    /// address`. The bridge was calling `run_batch_auth_slot`, the GUI's Excel
    /// batch pipeline, whose very first act is reading the MAC because the MAC
    /// is its lookup key into the spreadsheet row that holds the credentials.
    ///
    /// CoBuilder has no spreadsheet: the backend hands the bridge one specific
    /// uuid/authkey and one specific device. That is `FlashMode::Authorize` —
    /// the CLI's `authorize` command, dispatched by `registry::run_job` to
    /// `run_authorize` before any chip plugin is looked up — and it never asks
    /// the device for its MAC.
    #[test]
    fn an_authorization_writes_the_given_credentials_without_asking_for_a_mac() {
        let spec = AuthJobSpec {
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            baud_rate: 115_200,
            uuid: "uuidxxxxxxxxxxxxxxxx".to_string(),
            auth_key: "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
        };

        let job = authorize_job(&spec);

        assert!(
            matches!(job.mode, tyutool_core::FlashMode::Authorize),
            "the MAC-free single-device flow is the Authorize mode, not a batch slot"
        );
        assert_eq!(job.authorize_uuid.as_deref(), Some(spec.uuid.as_str()));
        assert_eq!(job.authorize_key.as_deref(), Some(spec.auth_key.as_str()));
        assert_eq!(job.port, spec.port);
        assert_eq!(job.chip_id, spec.chip_id);
        assert_eq!(job.baud_rate, spec.baud_rate);
        // KV is not the bridge's choice to make: `run_authorize` forces it for
        // single-device writes and ignores this field. Spelling it `None` keeps
        // the bridge from ever *looking* like it asked for the irreversible OTP
        // burn, which is a batch-only feature.
        assert_eq!(job.authorize_storage, None);
        // No callback = "already confirmed": the dangerous-op gate ran before
        // the job started, so a device holding other credentials is overwritten
        // (PRD 覆盖不可撤销) — the `ConflictPolicy::Overwrite` this flow had.
        assert!(job.confirm_overwrite.is_none());
        // Nothing that could send the job down a flash / erase / read branch.
        assert!(job.firmware_path.is_none());
        assert!(job.segments.is_none());
    }

    /// `run_authorize` reports what it found on the device through milestones,
    /// and two of them carry credentials in the clear: `AuthReadComplete`
    /// (the device's uuid + authkey) and `AuthConflict` (the credentials it is
    /// about to lose). The GUI shows those in a secure modal; the bridge has no
    /// business putting them on a WebSocket that a browser tab logs.
    ///
    /// The web client's SECURE_SILENT filter is not the guarantee here — that is
    /// the *other end* being careful. This is the bridge refusing to send them
    /// at all, which is the only half we control.
    #[test]
    fn credential_bearing_milestones_never_become_a_progress_frame() {
        for milestone in [
            tyutool_core::FlashMilestone::AuthReadComplete {
                uuid: "uuid-supersecret-9f1".to_string(),
                authkey: "authkey-supersecret-3c7".to_string(),
            },
            tyutool_core::FlashMilestone::AuthConflict {
                existing_uuid: "uuid-supersecret-9f1".to_string(),
                existing_authkey: "authkey-supersecret-3c7".to_string(),
            },
        ] {
            let payload = auth_progress_payload(&tyutool_core::FlashEvent::Milestone {
                milestone: milestone.clone(),
            });
            assert!(
                payload.is_none(),
                "{milestone:?} must not reach the client, got {payload:?}"
            );
        }

        // `Done` carries the failure prose, which is the other way a credential
        // could ride out on the progress channel. The bridge answers with its
        // own `job_result` frame, so this event has nothing to add anyway.
        let done = auth_progress_payload(&tyutool_core::FlashEvent::Done {
            result: tyutool_core::FlashResult::Err {
                message: "anything at all".to_string(),
                elapsed_secs: 1.0,
            },
        });
        assert!(done.is_none(), "got {done:?}");
    }

    /// `cancelled_after_write` is the protocol's only way of saying "this
    /// authorization code may already be spent" (PROTOCOL.md §取消后的设备状态
    /// forbids the client from saying 未写入 for it), so moving the bridge onto
    /// `run_authorize` may not quietly lose it.
    ///
    /// The dividing line is the same one the batch slot drew: has the write
    /// command left the bridge? Core announces that with `AuthWriteSent`, which
    /// is also the milestone the user is shown as `writing_auth`.
    #[test]
    fn a_cancel_after_the_write_command_reached_the_device_still_gets_its_own_code() {
        let spec = AuthJobSpec {
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            baud_rate: 115_200,
            uuid: "uuid-supersecret-9f1".to_string(),
            auth_key: "authkey-supersecret-3c7".to_string(),
        };
        let frames = std::sync::Mutex::new(Vec::<serde_json::Value>::new());
        let record = |payload: serde_json::Value| {
            frames.lock().expect("frames lock").push(payload);
        };

        let after_write = authorize_slot(&spec, &record, |job, on_event| {
            // What core is actually asked to run: the MAC-free single-device
            // authorization, carrying the request's own credentials.
            assert!(matches!(job.mode, tyutool_core::FlashMode::Authorize));
            assert_eq!(job.authorize_uuid.as_deref(), Some(spec.uuid.as_str()));
            on_event(tyutool_core::FlashEvent::Milestone {
                milestone: tyutool_core::FlashMilestone::AuthWriteSent,
            });
            Err(tyutool_core::FlashError::Cancelled)
        })
        .expect_err("a cancelled write is not a success");

        assert_eq!(after_write.error_code, "cancelled_after_write");
        assert!(
            after_write.message.contains(&spec.port),
            "the message must name the board in doubt: {}",
            after_write.message
        );
        // The message travels on the wire and into logs; a credential may not.
        assert!(
            !after_write.message.contains(&spec.uuid)
                && !after_write.message.contains(&spec.auth_key),
            "the message leaked a credential: {}",
            after_write.message
        );
        // Same milestone, seen from the client's side: the step that says the
        // write is under way.
        assert_eq!(
            frames.into_inner().expect("frames"),
            vec![serde_json::json!({ "step": "writing_auth" })]
        );

        // A cancel that landed before the write keeps the plain code.
        let clean = authorize_slot(&spec, &|_| {}, |_job, _on_event| {
            Err(tyutool_core::FlashError::Cancelled)
        })
        .expect_err("a cancellation is not a success");
        assert_eq!(clean.error_code, "cancelled");
    }

    #[test]
    fn debug_output_of_a_request_never_carries_a_credential() {
        // No log line prints these today, but "nobody prints it" is a review
        // promise, while a redacted Debug is a compile-time one: the next person
        // who adds `{message:?}` to a warning cannot leak a key by accident.
        let auth = WireAuth {
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            baud_rate: 921_600,
            uuid: "uuid-supersecret-9f1".to_string(),
            auth_key: "authkey-supersecret-3c7".to_string(),
        };
        let rendered = format!("{auth:?}");
        assert!(
            !rendered.contains("uuid-supersecret-9f1")
                && !rendered.contains("authkey-supersecret-3c7"),
            "WireAuth Debug leaked a credential: {rendered}"
        );
        // Still useful for diagnosis: the non-secret fields survive.
        assert!(
            rendered.contains("t5ai") && rendered.contains("/dev/tty.fakeA"),
            "{rendered}"
        );

        let spec = AuthJobSpec {
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            baud_rate: 921_600,
            uuid: "uuid-supersecret-9f1".to_string(),
            auth_key: "authkey-supersecret-3c7".to_string(),
        };
        let rendered = format!("{spec:?}");
        assert!(
            !rendered.contains("uuid-supersecret-9f1")
                && !rendered.contains("authkey-supersecret-3c7"),
            "AuthJobSpec Debug leaked a credential: {rendered}"
        );

        let message = ClientMessage::RunAuth {
            request_id: "a-1".to_string(),
            auth: WireAuth {
                chip_id: "t5ai".to_string(),
                port: "/dev/tty.fakeA".to_string(),
                baud_rate: 921_600,
                uuid: "uuid-supersecret-9f1".to_string(),
                auth_key: "authkey-supersecret-3c7".to_string(),
            },
        };
        let rendered = format!("{message:?}");
        assert!(
            !rendered.contains("uuid-supersecret-9f1")
                && !rendered.contains("authkey-supersecret-3c7"),
            "ClientMessage Debug leaked a credential: {rendered}"
        );

        // A multi-MB base64 image has no business in a log line either.
        let flash = ClientMessage::RunJob {
            request_id: "j-1".to_string(),
            job: serde_json::from_value(serde_json::json!({
                "chip_id": "t5ai",
                "port": "/dev/tty.fakeA",
                "baud_rate": 2_000_000
            }))
            .expect("build a WireJob"),
            file_content: "QUJDRA==".repeat(64),
        };
        let rendered = format!("{flash:?}");
        assert!(
            !rendered.contains("QUJDRA==QUJDRA=="),
            "ClientMessage Debug dumped the whole firmware payload: {rendered}"
        );
    }
    #[test]
    fn debug_output_of_a_grant_never_carries_its_token() {
        // The token is a bearer credential: presenting it is enough to write a
        // device. That makes it at least as sensitive as `auth_key`, which
        // already has a hand-written Debug — one stray `log::debug!("{grant:?}")`
        // or a `Vec<Grant>` in an error message would otherwise dump it whole.
        let grant = Grant {
            token: "Ab3-xYz_supersecret_token_value_0123456789a".to_string(),
            origin: "http://localhost:3000".to_string(),
            granted_at_ms: 1_784_800_000_000,
        };

        let rendered = format!("{grant:?}");

        assert!(
            !rendered.contains("supersecret"),
            "Grant Debug leaked the token: {rendered}"
        );
        // Not even a prefix: `redact_token`'s fingerprint exists for the audit
        // trail, where correlating two lines is the point. A Debug rendering has
        // no such need, so it gets the strict form.
        assert!(
            !rendered.contains("Ab3-xY"),
            "Grant Debug leaked a token prefix: {rendered}"
        );
        // Still useful: which origin, and that a token is present at all.
        assert!(
            rendered.contains("http://localhost:3000") && rendered.contains("len=43"),
            "Grant Debug lost its diagnostic value: {rendered}"
        );
    }
}
