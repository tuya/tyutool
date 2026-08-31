//! WebSocket dev-serve mode for tyutool-cli.
//! Exposes serial port operations over a local WebSocket so the Vite dev
//! server (localhost:1420) can flash real devices without the Tauri shell.
//!
//! Usage: tyutool-cli serve [--port 9527]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{Request, Response},
        http::{header, status::StatusCode},
        protocol::WebSocketConfig,
        Message,
    },
};
use tyutool_core::{
    create_serial_debug_state_resilient, device_reset_dtr_rts, list_serial_ports, run_job,
    serial_debug_fail_backfill_if_current, serial_debug_finalize_pending,
    serial_debug_finish_backfill_if_current, serial_debug_ingest_lines,
    serial_debug_scan_filter_matches, serial_debug_spawn_chunk_bridge, ArchivedChunk, DebugChunk,
    DebugConfig, FlashJob, SerialDebugArchiveReader, SerialDebugChunkBridgeHandle,
    SerialDebugFilterBackfillSnapshot, SerialDebugFilterDefinition, SerialDebugFilterPage,
    SerialDebugFilterStats, SerialDebugGeneration, SerialDebugSession, SerialDebugSessionPage,
    SerialDebugSink, SerialPortEntry,
};

// ── Client → Server ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ClientMessage {
    ListPorts,
    DeviceReset {
        port: String,
        chip_id: String,
    },
    RunJob {
        job: FlashJob,
        #[serde(default)]
        file_content: Option<String>,
        #[serde(default)]
        file_contents: Option<Vec<String>>,
    },
    Cancel,
    SerialDebugOpen {
        cfg: DebugConfig,
    },
    SerialDebugClose,
    SerialDebugDeviceReset {
        chip_id: String,
    },
    SerialDebugSend {
        bytes: Vec<u8>,
    },
    SerialDebugState,
    SerialDebugSessionClear,
    SerialDebugAppendSysLine {
        ts_ms: u64,
        text: String,
    },
    SerialDebugFilterAdd {
        keyword: String,
        use_regex: bool,
        color: String,
        #[serde(default)]
        request_id: Option<String>,
    },
    SerialDebugFilterRemove {
        filter_id: String,
    },
    SerialDebugFilterReadMatches {
        filter_id: String,
        start: u64,
        limit: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    SerialDebugSessionReadPage {
        start: u64,
        limit: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    /// Per-session archive byte cap. Without this the `dev:web` archive would
    /// stay unbounded and the setting would silently do nothing in web mode.
    SerialDebugSetArchiveLimit {
        max_bytes: u64,
    },
    /// Frontend response to `flash_progress` `auth_conflict` milestone.
    AuthorizeConfirm {
        confirmed: bool,
    },
}

// ── Server → Client ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ports {
        ports: Vec<SerialPortEntry>,
    },
    DeviceResetResult {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SerialDebugDeviceResetResult {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Progress {
        payload: serde_json::Value,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SerialDebugChunk {
        chunk: ArchivedChunk,
    },
    SerialDebugChunkBatch {
        chunks: Vec<ArchivedChunk>,
    },
    SerialDebugOpened,
    SerialDebugClosed,
    SerialDebugStateInfo {
        open: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        cfg: Option<DebugConfig>,
    },
    SerialDebugDisconnected {
        reason: String,
    },
    SerialDebugFilterUpdated {
        def: SerialDebugFilterDefinition,
        stats: SerialDebugFilterStats,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SerialDebugFilterPage {
        page: SerialDebugFilterPage,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SerialDebugSessionPage {
        page: SerialDebugSessionPage,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// The session archive stopped recording. The frontend renders the wording
    /// from `serialDebug.log.archiveCapped`, so only the number crosses.
    SerialDebugArchiveCapped {
        limit_mib: u64,
        /// `archived_before` of the cap sentinel itself — see [`ArchivedChunk`].
        archived_before: u64,
    },
    /// Device output was dropped because the bridge queue was full. One message
    /// per coalesced burst; the wording comes from
    /// `serialDebug.log.chunksDropped`.
    SerialDebugChunksDropped {
        dropped_bytes: u64,
        /// `archived_before` of the gap lines this notice belongs to — see
        /// [`ArchivedChunk`].
        archived_before: u64,
    },
}

/// Turns everything the shared chunk bridge produces into `ServerMessage`s.
///
/// Sends are fire-and-forget by contract ([`SerialDebugSink`]): the sink task
/// drains this channel and a closed WebSocket is normal teardown, so the bridge
/// thread must not stall on discovering it.
#[derive(Clone)]
struct WsSink {
    tx: tokio::sync::mpsc::UnboundedSender<ServerMessage>,
}

impl SerialDebugSink for WsSink {
    fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
        let _ = self
            .tx
            .send(ServerMessage::SerialDebugChunkBatch { chunks });
    }

    fn chunks_dropped(&self, dropped_bytes: u64, archived_before: u64) {
        let _ = self.tx.send(ServerMessage::SerialDebugChunksDropped {
            dropped_bytes,
            archived_before,
        });
    }

    fn archive_capped(&self, limit_mib: u64, archived_before: u64) {
        let _ = self.tx.send(ServerMessage::SerialDebugArchiveCapped {
            limit_mib,
            archived_before,
        });
    }

    fn filter_updated(&self, def: SerialDebugFilterDefinition, stats: SerialDebugFilterStats) {
        let _ = self.tx.send(ServerMessage::SerialDebugFilterUpdated {
            def,
            stats,
            request_id: None,
        });
    }
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn run_serve(port: u16) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    println!("tyutool-cli serve listening on ws://{addr}");
    println!("Press Ctrl+C to stop.");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                log::info!("WS connection from {peer}");
                tokio::spawn(handle_connection(stream));
            }
            Err(e) => log::warn!("accept error: {e}"),
        }
    }
}

// ── WS 握手来源校验（自上游 tyutool a2fa599 移植）────────────────────────────
//
// ⚠ 这段是**逐字取自上游**的，不要重写：它挡的是一个真实攻击面——任意网页
// （如 https://evil.com）都能向 ws://127.0.0.1:<port> 发起连接，连上之后就能驱动
// 刷写 / 擦除 / 复位用户手上的硬件。安全代码自己重写等于重新引入 bug。
//
// 为什么在这里而不是在 crates/tyutool-cli/src/serve.rs：本 fork 把 dev-serve 的实现
// 抽成了这个共享 crate（cli 那侧只剩一行 re-export），上游那笔补丁打在原文件上，
// 合并时落不进来。**这类「上游改了原文件、而我们把实现搬走了」的补丁，每次同步
// 上游都要单独核对一遍**，自动合并不会替你发现。
// 上游对应提交：a2fa599 fix(cli): gate dev-serve WebSocket by Origin/Host and cap message size

/// Hosts considered local enough to drive the dev-serve WS. Any cross-origin
/// browser page (e.g. `https://evil.com`) reaching `ws://127.0.0.1:<port>` must
/// be refused so it cannot run flash/erase/reset on the user's hardware.
const LOCAL_WS_HOSTS: &[&str] = &["127.0.0.1", "localhost", "[::1]"];

/// Validate the WebSocket handshake request's `Origin` and `Host` headers.
///
/// - `Host` must be a loopback host (`127.0.0.1` / `localhost` / `[::1]`),
///   defeating DNS-rebinding attacks where `evil.com` resolves to 127.0.0.1.
/// - `Origin`, when present, must be absent (non-browser client) or point at a
///   local scheme/host, so a malicious web page can't connect.
///
/// Returns `Ok(response)` to accept, or `Err(error_response)` to reject with
/// HTTP 403.
///
/// `clippy::result_large_err`: the `Err` variant (`ErrorResponse =
/// Response<Option<String>>`) is ~136 bytes, but its type is fixed by
/// tungstenite's `Callback` trait, so it cannot be boxed here.
#[allow(clippy::result_large_err)]
fn validate_ws_origin(
    req: &Request,
    response: Response,
) -> Result<Response, tokio_tungstenite::tungstenite::http::Response<Option<String>>> {
    let headers = req.headers();

    // Host check — reject DNS rebinding.
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host_lower = host.to_lowercase();
    let host_ok = LOCAL_WS_HOSTS
        .iter()
        .any(|allowed| host_lower == *allowed || host_lower.starts_with(&format!("{allowed}:")));
    if !host_ok {
        log::warn!("WS reject: non-loopback Host header: {host:?}");
        return Err(forbidden("host not allowed"));
    }

    // Origin check — if a browser sends it, it must be a local origin.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let origin_lower = origin.to_lowercase();
        let origin_ok = origin_lower.starts_with("tauri://")
            || LOCAL_WS_HOSTS.iter().any(|allowed| {
                origin_lower == format!("http://{allowed}")
                    || origin_lower.starts_with(&format!("http://{allowed}:"))
                    || origin_lower == format!("https://{allowed}")
                    || origin_lower.starts_with(&format!("https://{allowed}:"))
            });
        if !origin_ok {
            log::warn!("WS reject: cross-origin request: Origin={origin:?}");
            return Err(forbidden("origin not allowed"));
        }
    }
    // No Origin header → non-browser client (e.g. the Vite dev proxy). Allowed.

    Ok(response)
}

/// Build an HTTP 403 error response to reject a WS handshake.
fn forbidden(reason: &str) -> tokio_tungstenite::tungstenite::http::Response<Option<String>> {
    tokio_tungstenite::tungstenite::http::Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Some(reason.to_string()))
        .expect("building a static 403 response cannot fail")
}

// ── Per-connection handler ───────────────────────────────────────────────────

async fn handle_connection(stream: tokio::net::TcpStream) {
    // Cap WS message size at 16 MiB (default is 64 MiB) to bound per-connection
    // memory amplification from a malicious client streaming a large base64 blob.
    // WebSocketConfig is #[non_exhaustive] since tungstenite 0.26 — set fields
    // through the builder, not a struct literal.
    let ws_config = WebSocketConfig::default().max_message_size(Some(16 * 1024 * 1024));
    let ws = match accept_hdr_async_with_config(stream, validate_ws_origin, Some(ws_config)).await {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("WS handshake failed: {e}");
            return;
        }
    };

    let (sink, mut stream) = ws.split();
    let cancel = Arc::new(AtomicBool::new(false));
    let pending_confirm: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>> =
        Arc::new(Mutex::new(None));
    // The in-flight flash job, if any.
    //
    // It runs on its own task rather than being awaited inside the message loop
    // below. Awaiting it there meant the loop could not read the socket for the
    // whole duration of a job — so `Cancel` and `AuthorizeConfirm`, the only two
    // messages a client has any reason to send *during* a job, sat unread in the
    // receive buffer. Cancel silently did nothing, and an authorize run that hit
    // a credential conflict deadlocked: the blocking thread waited for a
    // confirmation that could never be read.
    let mut running_job: Option<tokio::task::JoinHandle<()>> = None;

    // ── mpsc sink pump ───────────────────────────────────────────────────────
    // Background tasks (progress callbacks, serial-debug reader thread) need to
    // push ServerMessage values from contexts that don't own the WS sink. Wrap
    // the sink in a single drainer task fed by an unbounded mpsc.
    use tokio::sync::mpsc;
    let (sink_tx, mut sink_rx) = mpsc::unbounded_channel::<ServerMessage>();
    // The chunk bridge and every archive-ingest path share one sink for the
    // life of the connection; it is just a channel handle.
    let ws_sink = WsSink {
        tx: sink_tx.clone(),
    };

    let mut sink_moved = sink;
    let pump = tokio::spawn(async move {
        while let Some(msg) = sink_rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap_or_else(|e| {
                serde_json::to_string(&ServerMessage::Error {
                    message: e.to_string(),
                    request_id: None,
                })
                .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"serialize failed\"}".into())
            });
            if sink_moved.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // Per-connection serial-debug session state.
    let debug_session: Arc<Mutex<Option<SerialDebugSession>>> = Arc::new(Mutex::new(None));
    let debug_chunk_bridge: Arc<Mutex<Option<SerialDebugChunkBridgeHandle>>> =
        Arc::new(Mutex::new(None));
    let debug_generation = Arc::new(SerialDebugGeneration::default());
    let serial_debug_dir = std::env::temp_dir().join("tyutool").join("serial-debug");
    // ⚠ 用 resilient 版本而不是 .expect()：见该函数上方的移植说明。
    // 返回值里的目录是**实际用上的那个**（可能是 pid- 兜底子目录），后续历史查询要用它，
    // 别再用上面那个 primary。
    let (serial_debug_dir, debug_archive_inner, debug_filters_inner) =
        create_serial_debug_state_resilient(&serial_debug_dir);
    let debug_archive = Arc::new(Mutex::new(debug_archive_inner));
    let debug_filters = Arc::new(Mutex::new(debug_filters_inner));

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: e.to_string(),
                    request_id: None,
                });
                continue;
            }
        };

        match client_msg {
            ClientMessage::ListPorts => {
                let ports = list_serial_ports().unwrap_or_default();
                let _ = sink_tx.send(ServerMessage::Ports { ports });
            }
            ClientMessage::DeviceReset { port, chip_id } => {
                // Run on blocking pool so serial `open()` / DTR/RTS never stalls the WS task.
                let join =
                    tokio::task::spawn_blocking(move || device_reset_dtr_rts(&port, &chip_id));
                let outcome = tokio::time::timeout(Duration::from_secs(15), join).await;
                let msg = match outcome {
                    Ok(Ok(Ok(()))) => ServerMessage::DeviceResetResult {
                        ok: true,
                        error: None,
                    },
                    Ok(Ok(Err(e))) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(e.to_string()),
                    },
                    Ok(Err(join_e)) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(format!("reset task join: {join_e}")),
                    },
                    Err(_elapsed) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(
                            "device reset timed out (serial port blocked or driver hung)".into(),
                        ),
                    },
                };
                let _ = sink_tx.send(msg);
            }
            ClientMessage::Cancel => {
                cancel.store(true, Ordering::Relaxed);
                // Wake any thread blocked in confirm_overwrite so it can return Cancelled.
                let mut g = pending_confirm.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(tx) = g.take() {
                    let _ = tx.send(false);
                }
            }
            ClientMessage::RunJob {
                job,
                file_content,
                file_contents,
            } => {
                // One job at a time per connection. Previously this was implicit
                // — the loop blocked on the job, so a second one could not be
                // read — and clearing the shared cancel flag below was safe only
                // because of that. Now that the loop keeps running, the limit has
                // to be stated: without it, a second job would reset the flag out
                // from under the first.
                if running_job.as_ref().is_some_and(|h| !h.is_finished()) {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "a job is already running on this connection".into(),
                        request_id: None,
                    });
                    continue;
                }
                cancel.store(false, Ordering::Relaxed);
                running_job = Some(tokio::spawn(handle_run_job(
                    sink_tx.clone(),
                    Arc::clone(&cancel),
                    job,
                    file_content,
                    file_contents,
                    Arc::clone(&pending_confirm),
                )));
            }
            ClientMessage::SerialDebugOpen { cfg } => {
                let mut guard = debug_session.lock().unwrap();
                if guard.is_some() {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "already open".into(),
                        request_id: None,
                    });
                    continue;
                }
                let sink_for_disc = sink_tx.clone();
                let chunk_bridge = serial_debug_spawn_chunk_bridge(
                    ws_sink.clone(),
                    Arc::clone(&debug_archive),
                    Arc::clone(&debug_filters),
                    Arc::clone(&debug_generation),
                );
                let chunk_bridge_for_session = chunk_bridge.clone();
                let result = SerialDebugSession::open(
                    cfg,
                    Box::new(move |chunk| {
                        chunk_bridge_for_session.send_chunk(chunk);
                    }),
                    Box::new(move |reason| {
                        let _ =
                            sink_for_disc.send(ServerMessage::SerialDebugDisconnected { reason });
                    }),
                );
                match result {
                    Ok(s) => {
                        *guard = Some(s);
                        *debug_chunk_bridge.lock().unwrap() = Some(chunk_bridge);
                        let _ = sink_tx.send(ServerMessage::SerialDebugOpened);
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                            request_id: None,
                        });
                    }
                }
            }
            ClientMessage::SerialDebugClose => {
                let mut guard = debug_session.lock().unwrap();
                if let Some(s) = guard.take() {
                    s.close();
                }
                if let Some(bridge) = debug_chunk_bridge.lock().unwrap().take() {
                    let _ = bridge.shutdown();
                }
                // After the bridge's shutdown ack, so everything it was still
                // holding is archived and the tail is cut behind all of it.
                let lines = serial_debug_finalize_pending(&debug_archive);
                serial_debug_ingest_lines(&ws_sink, &debug_filters, &lines);
                let _ = sink_tx.send(ServerMessage::SerialDebugClosed);
            }
            ClientMessage::SerialDebugDeviceReset { chip_id } => {
                let outcome = {
                    let guard = debug_session.lock().unwrap();
                    match guard.as_ref() {
                        Some(session) => session.device_reset(&chip_id),
                        None => Err(tyutool_core::FlashError::Plugin(
                            "serial debug not open".into(),
                        )),
                    }
                };
                let _ = sink_tx.send(match outcome {
                    Ok(()) => ServerMessage::SerialDebugDeviceResetResult {
                        ok: true,
                        error: None,
                    },
                    Err(err) => ServerMessage::SerialDebugDeviceResetResult {
                        ok: false,
                        error: Some(err.to_string()),
                    },
                });
            }
            ClientMessage::SerialDebugSend { bytes } => {
                let guard = debug_session.lock().unwrap();
                if let Some(s) = guard.as_ref() {
                    if let Err(e) = s.write(&bytes) {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                            request_id: None,
                        });
                        continue;
                    }
                    let ts_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let chunk = DebugChunk {
                        direction: tyutool_core::Direction::Tx,
                        ts_ms,
                        bytes,
                    };
                    // Tx chunks bypass the bounded bridge queue and are archived
                    // right here, so this is where `archived_before` has to be
                    // read (same lock guard).
                    let (completed, archived_before) = {
                        let mut archive = debug_archive.lock().unwrap();
                        let archived_before = archive.total_lines();
                        (
                            archive.append_chunk(&chunk).unwrap_or_default(),
                            archived_before,
                        )
                    };
                    serial_debug_ingest_lines(&ws_sink, &debug_filters, &completed);
                    let _ = sink_tx.send(ServerMessage::SerialDebugChunk {
                        chunk: ArchivedChunk {
                            chunk,
                            archived_before,
                        },
                    });
                } else {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "serial debug not open".into(),
                        request_id: None,
                    });
                }
            }
            ClientMessage::SerialDebugState => {
                let guard = debug_session.lock().unwrap();
                let (open, cfg) = match guard.as_ref() {
                    Some(s) => (true, Some(s.config().clone())),
                    None => (false, None),
                };
                let _ = sink_tx.send(ServerMessage::SerialDebugStateInfo { open, cfg });
            }
            ClientMessage::SerialDebugSessionClear => {
                if let Some(bridge) = debug_chunk_bridge.lock().unwrap().as_ref().cloned() {
                    if let Err(message) = bridge.reset() {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message,
                            request_id: None,
                        });
                        continue;
                    }
                } else {
                    debug_generation.advance();
                }
                {
                    let mut archive = debug_archive.lock().unwrap();
                    let _ = archive.clear();
                }
                let updates = debug_filters.lock().unwrap().reset_for_new_session();
                let filters = debug_filters.lock().unwrap();
                for stats in updates {
                    if let Some(def) = filters.definition(&stats.filter_id) {
                        let _ = sink_tx.send(ServerMessage::SerialDebugFilterUpdated {
                            def,
                            stats,
                            request_id: None,
                        });
                    }
                }
            }
            ClientMessage::SerialDebugAppendSysLine { ts_ms, text } => {
                let line = {
                    let mut archive = debug_archive.lock().unwrap();
                    match archive.append_sys_line(ts_ms, text) {
                        Ok(line) => line,
                        Err(e) => {
                            let _ = sink_tx.send(ServerMessage::Error {
                                message: e.to_string(),
                                request_id: None,
                            });
                            continue;
                        }
                    }
                };
                // `None` once the session archive hit its size cap.
                if let Some(line) = line {
                    serial_debug_ingest_lines(&ws_sink, &debug_filters, &[line]);
                }
            }
            ClientMessage::SerialDebugSetArchiveLimit { max_bytes } => {
                debug_archive.lock().unwrap().set_max_bytes(max_bytes);
            }
            ClientMessage::SerialDebugFilterAdd {
                keyword,
                use_regex,
                color,
                request_id,
            } => {
                let snapshot_total_lines = debug_archive.lock().unwrap().total_lines();
                let current_generation = debug_generation.current();
                let def = match debug_filters.lock().unwrap().add_filter(
                    keyword,
                    use_regex,
                    color,
                    snapshot_total_lines,
                ) {
                    Ok(def) => def,
                    Err(message) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message,
                            request_id,
                        });
                        continue;
                    }
                };
                let initial = debug_filters.lock().unwrap().stats(&def.id).unwrap();
                let _ = sink_tx.send(ServerMessage::SerialDebugFilterUpdated {
                    def: def.clone(),
                    stats: initial,
                    request_id: request_id.clone(),
                });
                let (backfill_stats, backfill_snapshot, archive_reader): (
                    SerialDebugFilterStats,
                    SerialDebugFilterBackfillSnapshot,
                    SerialDebugArchiveReader,
                ) = {
                    let mut filters = debug_filters.lock().unwrap();
                    let stats = match filters.start_backfill(&def.id) {
                        Ok(stats) => stats,
                        Err(e) => {
                            let _ = sink_tx.send(ServerMessage::Error {
                                message: e.to_string(),
                                request_id,
                            });
                            continue;
                        }
                    };
                    let snapshot = match filters.backfill_snapshot(&def.id) {
                        Some(snapshot) => snapshot,
                        None => {
                            let _ = sink_tx.send(ServerMessage::Error {
                                message: "new filter backfill snapshot missing".into(),
                                request_id,
                            });
                            continue;
                        }
                    };
                    drop(filters);
                    let archive = debug_archive.lock().unwrap();
                    (stats, snapshot, archive.snapshot_reader())
                };
                let _ = sink_tx.send(ServerMessage::SerialDebugFilterUpdated {
                    def: def.clone(),
                    stats: backfill_stats,
                    request_id: None,
                });

                let sink_for_backfill = sink_tx.clone();
                let filters_for_backfill = Arc::clone(&debug_filters);
                let generation_for_backfill = Arc::clone(&debug_generation);
                let filter_id = def.id.clone();
                let historical_idx_path = serial_debug_dir
                    .join(format!("serial-debug-filter-{filter_id}.historical.idx"));
                tokio::task::spawn_blocking(move || {
                    let result = serial_debug_scan_filter_matches(
                        &backfill_snapshot,
                        &archive_reader,
                        &historical_idx_path,
                    );
                    let mut filters = match filters_for_backfill.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    let stats = match result {
                        Ok((historical_match_count, historical_scanned_until_line_no)) => {
                            match serial_debug_finish_backfill_if_current(
                                &generation_for_backfill,
                                current_generation,
                                &mut filters,
                                &filter_id,
                                &historical_idx_path,
                                historical_match_count,
                                historical_scanned_until_line_no,
                            ) {
                                Ok(stats) => stats,
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                                Err(e) => serial_debug_fail_backfill_if_current(
                                    &generation_for_backfill,
                                    current_generation,
                                    &mut filters,
                                    &filter_id,
                                    e.to_string(),
                                )
                                .ok()
                                .flatten(),
                            }
                        }
                        Err(e) => serial_debug_fail_backfill_if_current(
                            &generation_for_backfill,
                            current_generation,
                            &mut filters,
                            &filter_id,
                            e.to_string(),
                        )
                        .ok()
                        .flatten(),
                    };
                    let _ = std::fs::remove_file(&historical_idx_path);
                    if let Some(stats) = stats {
                        if let Some(def) = filters.definition(&filter_id) {
                            let _ =
                                sink_for_backfill.send(ServerMessage::SerialDebugFilterUpdated {
                                    def,
                                    stats,
                                    request_id: None,
                                });
                        }
                    }
                });
            }
            ClientMessage::SerialDebugFilterRemove { filter_id } => {
                if !debug_filters.lock().unwrap().remove_filter(&filter_id) {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "filter not found".into(),
                        request_id: None,
                    });
                }
            }
            ClientMessage::SerialDebugFilterReadMatches {
                filter_id,
                start,
                limit,
                request_id,
            } => {
                let archive = debug_archive.lock().unwrap();
                let filters = debug_filters.lock().unwrap();
                match filters.read_match_page(&filter_id, start, limit, &archive) {
                    Ok(page) => {
                        let _ =
                            sink_tx.send(ServerMessage::SerialDebugFilterPage { page, request_id });
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                            request_id,
                        });
                    }
                }
            }
            ClientMessage::SerialDebugSessionReadPage {
                start,
                limit,
                request_id,
            } => {
                let archive = debug_archive.lock().unwrap();
                match archive.read_page(start, limit) {
                    Ok(page) => {
                        let _ = sink_tx
                            .send(ServerMessage::SerialDebugSessionPage { page, request_id });
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                            request_id,
                        });
                    }
                }
            }
            ClientMessage::AuthorizeConfirm { confirmed } => {
                let mut guard = pending_confirm.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(tx) = guard.take() {
                    let _ = tx.send(confirmed);
                }
            }
        }
    }

    // Best-effort: wake any auth thread blocked on confirm_overwrite, and signal cancel.
    // Without this, a WS disconnect during an AuthConflict prompt would park the
    // blocking thread forever (holding the serial port open).
    cancel.store(true, Ordering::Relaxed);
    {
        let mut g = pending_confirm.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = g.take() {
            let _ = tx.send(false);
        }
    }

    // Let the cancelled job finish unwinding before the sink goes away, so the
    // serial port is released and its task does not outlive the connection.
    if let Some(handle) = running_job.take() {
        let _ = handle.await;
    }

    // Clean up any open serial-debug session before dropping the sink.
    if let Ok(mut guard) = debug_session.lock() {
        if let Some(s) = guard.take() {
            s.close();
        }
    }
    if let Ok(mut guard) = debug_chunk_bridge.lock() {
        if let Some(bridge) = guard.take() {
            let _ = bridge.shutdown();
        }
    }

    drop(sink_tx);
    let _ = pump.await;

    log::info!("WS connection closed");
}

// ── Run job handler ──────────────────────────────────────────────────────────

/// Takes its sender and job by value because it runs on a task of its own — see
/// `running_job` in [`handle_connection`] for why it must not be awaited inline.
async fn handle_run_job(
    sink_tx: tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    cancel: Arc<AtomicBool>,
    mut job: FlashJob,
    file_content: Option<String>,
    file_contents: Option<Vec<String>>,
    pending_confirm: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
) {
    let mut temp_paths: Vec<String> = Vec::new();

    // Decode single base64 firmware (legacy/read mode source)
    if let Some(b64) = file_content {
        match decode_to_temp(&b64, "tyutool_fw") {
            Ok(p) => {
                job.firmware_path = Some(p.clone());
                temp_paths.push(p);
            }
            Err(e) => {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: e.to_string(),
                    request_id: None,
                });
                return;
            }
        }
    }

    // Decode multiple base64 firmwares for multi-segment flashing
    if let Some(contents) = file_contents {
        if let Some(ref mut segments) = job.segments {
            if contents.len() != segments.len() {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: "file_contents length mismatch with segments".into(),
                    request_id: None,
                });
                return;
            }
            for (i, b64) in contents.iter().enumerate() {
                match decode_to_temp(b64, &format!("tyutool_seg_{i}")) {
                    Ok(p) => {
                        segments[i].firmware_path = p.clone();
                        temp_paths.push(p);
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                            request_id: None,
                        });
                        // cleanup already created files? temp_paths will be cleaned at end
                        return;
                    }
                }
            }
        }
    }

    // For read mode: use temp path when client path is empty/absent
    let is_read = matches!(job.mode, tyutool_core::FlashMode::Read);
    if is_read && job.read_file_path.as_deref().unwrap_or("").is_empty() {
        job.read_file_path = Some(temp_path("tyutool_read", "bin"));
    }
    let read_path = if is_read {
        job.read_file_path.clone()
    } else {
        None
    };

    let mut job_clone = job.clone();

    // Inject confirm_overwrite AFTER cloning because Clone resets it to None
    // (see impl Clone for FlashJob in crates/tyutool-core/src/job.rs).
    let sink_for_confirm = sink_tx.clone();
    let confirm_store = Arc::clone(&pending_confirm);
    job_clone.confirm_overwrite = Some(Box::new(move |existing_uuid, existing_authkey| {
        use tyutool_core::{FlashEvent, FlashMilestone};
        if let Ok(v) = serde_json::to_value(FlashEvent::Milestone {
            milestone: FlashMilestone::AuthConflict {
                existing_uuid,
                existing_authkey,
            },
        }) {
            let _ = sink_for_confirm.send(ServerMessage::Progress { payload: v });
        }
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        {
            let mut guard = confirm_store.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(tx);
        }
        rx.recv().unwrap_or(false)
    }));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();

    // Run job in a blocking thread; collect file output there too.
    let handle = tokio::task::spawn_blocking(move || {
        let result = run_job(&job_clone, &cancel, |p| {
            if let Ok(v) = serde_json::to_value(&p) {
                let _ = tx.send(v);
            }
        });

        // Read output file inside the blocking thread before returning
        let file_b64 = if let Some(ref path) = read_path {
            let b64 = std::fs::read(path)
                .ok()
                .map(|b| base64::engine::general_purpose::STANDARD.encode(&b));
            let _ = std::fs::remove_file(path);
            b64
        } else {
            None
        };

        (result, file_b64)
    });

    // Forward all progress from `run_job` (including `Done`). Only synthesize `Done` below if
    // the job never emitted one (e.g. serialization skip) — otherwise Authorize + `Ok(())`
    // after `run_job` returned `Ok(())` would incorrectly send `{ ok: true }`.
    let mut saw_done = false;
    while let Some(payload) = rx.recv().await {
        if payload
            .get("kind")
            .and_then(|k| k.as_str())
            .is_some_and(|k| k == "done")
        {
            saw_done = true;
        }
        let _ = sink_tx.send(ServerMessage::Progress { payload });
    }
    // Channel closed → blocking task has finished

    let (result, file_b64) = handle.await.unwrap_or((
        Err(tyutool_core::FlashError::Plugin(
            "task panicked".to_string(),
        )),
        None,
    ));

    // For read mode: send file_content message before Done
    if let Some(b64) = file_b64 {
        let path_str = job.read_file_path.as_deref().unwrap_or("read.bin");
        let name = std::path::Path::new(path_str)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("read.bin");
        let payload = serde_json::json!({ "kind": "file_content", "name": name, "content": b64 });
        let _ = sink_tx.send(ServerMessage::Progress { payload });
    }

    if !saw_done {
        let (ok, message) = match &result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        let done_payload = serde_json::json!({ "kind": "done", "ok": ok, "message": message });
        let _ = sink_tx.send(ServerMessage::Progress {
            payload: done_payload,
        });
    }

    // Clean up temp files
    for p in temp_paths {
        let _ = std::fs::remove_file(p);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn decode_to_temp(b64: &str, prefix: &str) -> anyhow::Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode failed")?;
    let path = temp_path(prefix, "bin");
    std::fs::write(&path, &bytes).context("write temp file")?;
    Ok(path)
}

fn temp_path(prefix: &str, ext: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir()
        .join(format!("{prefix}_{ts}.{ext}"))
        .to_string_lossy()
        .to_string()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Only the tests drive the shared bridge directly; production code reaches it
    // through `serial_debug_spawn_chunk_bridge`, so these stay out of the crate's
    // top-level imports.
    use tyutool_core::{
        serial_debug_flush_chunks, serial_debug_report_drops, Direction, SerialDebugArchive,
        SerialDebugChunkBatchBuffer, SerialDebugDropReport, SerialDebugFilterIndex,
    };

    // ── WS 来源校验用例（随实现一并自上游 a2fa599 移植）──────────────────────
    // ⚠ 实现移植了、用例没移植 = 防护没有任何证明，下次重构谁都不知道自己破坏了什么。
    // 六条覆盖：放行（无 Origin 的非浏览器客户端 / localhost / tauri://）与
    // 拒绝（跨源网页 / DNS rebinding 的 Host / 缺 Host 头）。
    use tokio_tungstenite::tungstenite::http::{
        header, Request as HttpRequest, Response as HttpResponse,
    };

    fn make_request(host: &str, origin: Option<&str>) -> Request {
        let mut builder = HttpRequest::builder()
            .method("GET")
            .uri("/")
            .header(header::HOST, host);
        if let Some(o) = origin {
            builder = builder.header(header::ORIGIN, o);
        }
        builder.body(()).unwrap()
    }

    fn ok_response() -> Response {
        HttpResponse::new(())
    }

    #[test]
    fn ws_accepts_loopback_host_without_origin() {
        // Non-browser client (no Origin), loopback Host → accept.
        let req = make_request("127.0.0.1:9527", None);
        assert!(validate_ws_origin(&req, ok_response()).is_ok());
    }

    #[test]
    fn ws_accepts_localhost_origin() {
        let req = make_request("localhost:9527", Some("http://localhost:5173"));
        assert!(validate_ws_origin(&req, ok_response()).is_ok());
    }

    #[test]
    fn ws_accepts_tauri_origin() {
        let req = make_request("127.0.0.1:9527", Some("tauri://localhost"));
        assert!(validate_ws_origin(&req, ok_response()).is_ok());
    }

    #[test]
    fn ws_rejects_cross_origin_browser_page() {
        // A malicious web page (evil.com) connecting to the local WS.
        let req = make_request("127.0.0.1:9527", Some("https://evil.example.com"));
        let err = validate_ws_origin(&req, ok_response()).unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn ws_rejects_dns_rebinding_host() {
        // evil.com resolves to 127.0.0.1 but the Host header betrays it.
        let req = make_request("evil.example.com:9527", None);
        let err = validate_ws_origin(&req, ok_response()).unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn ws_rejects_missing_host_header() {
        let req = HttpRequest::builder()
            .method("GET")
            .uri("/")
            .body(())
            .unwrap();
        let err = validate_ws_origin(&req, ok_response()).unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    use super::*;

    #[test]
    fn deserialize_list_ports() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"list_ports"}"#).unwrap();
        assert!(matches!(msg, ClientMessage::ListPorts));
    }

    #[test]
    fn deserialize_device_reset() {
        let msg: ClientMessage = serde_json::from_str(
            r#"{"type":"device_reset","port":"/dev/ttyUSB0","chip_id":"T5AI"}"#,
        )
        .unwrap();
        assert!(matches!(msg, ClientMessage::DeviceReset { .. }));
    }

    #[test]
    fn deserialize_serial_debug_set_archive_limit() {
        let msg: ClientMessage = serde_json::from_str(
            r#"{"type":"serial_debug_set_archive_limit","max_bytes":268435456}"#,
        )
        .unwrap();
        match msg {
            ClientMessage::SerialDebugSetArchiveLimit { max_bytes } => {
                assert_eq!(max_bytes, 268_435_456)
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn deserialize_serial_debug_device_reset() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_device_reset","chip_id":"T5AI"}"#)
                .unwrap();
        assert!(matches!(msg, ClientMessage::SerialDebugDeviceReset { .. }));
    }

    #[test]
    fn device_reset_result_wire_type_is_snake_case() {
        let msg = ServerMessage::DeviceResetResult {
            ok: true,
            error: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(
            s.contains(r#""type":"device_reset_result""#),
            "unexpected JSON (client listens for type device_reset_result): {s}"
        );
    }

    #[test]
    fn serial_debug_device_reset_result_wire_type_is_snake_case() {
        let msg = ServerMessage::SerialDebugDeviceResetResult {
            ok: true,
            error: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(
            s.contains(r#""type":"serial_debug_device_reset_result""#),
            "unexpected JSON (client listens for type serial_debug_device_reset_result): {s}"
        );
    }

    #[test]
    fn ports_message_keeps_usb_metadata() {
        let msg = ServerMessage::Ports {
            ports: vec![SerialPortEntry {
                path: "/dev/ttyACM0".into(),
                name: Some("USB Enhanced Serial".into()),
                usb_vid: Some(0x1a86),
                usb_pid: Some(0x55d2),
                usb_serial: None,
                usb_interface: None,
                port_role: None,
            }],
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""type":"ports""#), "unexpected JSON: {s}");
        assert!(
            s.contains(r#""usbVid":6790"#),
            "usb VID metadata missing: {s}"
        );
        assert!(
            s.contains(r#""usbPid":21970"#),
            "usb PID metadata missing: {s}"
        );
    }

    #[test]
    fn deserialize_cancel() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"cancel"}"#).unwrap();
        assert!(matches!(msg, ClientMessage::Cancel));
    }

    #[test]
    fn deserialize_run_job_minimal() {
        let json = r#"{
            "type": "run_job",
            "job": {
                "mode": "erase",
                "chipId": "T5AI",
                "port": "/dev/ttyUSB0",
                "baudRate": 921600,
                "eraseStartHex": "0x00000000",
                "eraseEndHex": "0x00200000"
            }
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::RunJob { .. }));
    }

    #[test]
    fn decode_to_temp_roundtrip() {
        let original = b"hello firmware";
        let b64 = base64::engine::general_purpose::STANDARD.encode(original);
        let path = decode_to_temp(&b64, "test_fw").unwrap();
        let read_back = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_back, original);
    }

    #[test]
    fn deserialize_serial_debug_open_with_config() {
        let json = r#"{
            "type": "serial_debug_open",
            "cfg": {
                "port": "/dev/ttyUSB0",
                "baudRate": 115200,
                "dataBits": "eight",
                "parity": "none",
                "stopBits": "one"
            }
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SerialDebugOpen { cfg } => {
                assert_eq!(cfg.port, "/dev/ttyUSB0");
                assert_eq!(cfg.baud_rate, 115200);
            }
            other => panic!("expected SerialDebugOpen, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_serial_debug_close_and_state() {
        let close: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_close"}"#).unwrap();
        assert!(matches!(close, ClientMessage::SerialDebugClose));
        let state: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_state"}"#).unwrap();
        assert!(matches!(state, ClientMessage::SerialDebugState));
    }

    #[test]
    fn deserialize_serial_debug_send_bytes() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_send","bytes":[1,2,255]}"#).unwrap();
        match msg {
            ClientMessage::SerialDebugSend { bytes } => assert_eq!(bytes, vec![1, 2, 255]),
            other => panic!("expected SerialDebugSend, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_authorize_confirm() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"authorize_confirm","confirmed":true}"#).unwrap();
        match msg {
            ClientMessage::AuthorizeConfirm { confirmed } => assert!(confirmed),
            other => panic!("expected AuthorizeConfirm, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_run_job_with_multiple_segments_and_file_contents() {
        let json = r#"{
            "type": "run_job",
            "job": {
                "mode": "flash",
                "chipId": "T5AI",
                "port": "/dev/ttyUSB0",
                "baudRate": 921600,
                "segments": [
                    {"firmwarePath": "", "startAddr": "0x00000000", "endAddr": "0x00100000"},
                    {"firmwarePath": "", "startAddr": "0x00100000", "endAddr": "0x00200000"}
                ]
            },
            "file_contents": ["YWFh", "YmJi"]
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::RunJob {
                job,
                file_content,
                file_contents,
            } => {
                assert!(file_content.is_none());
                let segments = job.segments.expect("segments present");
                assert_eq!(segments.len(), 2);
                assert_eq!(segments[1].start_addr, "0x00100000");
                assert_eq!(file_contents.unwrap().len(), 2);
            }
            other => panic!("expected RunJob, got {other:?}"),
        }
    }

    #[test]
    fn serialize_serial_debug_lifecycle_messages_use_snake_case() {
        assert!(serde_json::to_string(&ServerMessage::SerialDebugOpened)
            .unwrap()
            .contains(r#""type":"serial_debug_opened""#));
        assert!(serde_json::to_string(&ServerMessage::SerialDebugClosed)
            .unwrap()
            .contains(r#""type":"serial_debug_closed""#));
        let disc = serde_json::to_string(&ServerMessage::SerialDebugDisconnected {
            reason: "device removed".into(),
        })
        .unwrap();
        assert!(disc.contains(r#""type":"serial_debug_disconnected""#));
        assert!(disc.contains(r#""reason":"device removed""#));
    }

    /// *When* the notice fires is core's business and core tests it (only the
    /// archive's own `Sys` sentinel counts, never identical text from the
    /// device). What has to be pinned here is the shape it reaches the browser
    /// in, because `ws-transport.ts` reads these exact field names.
    #[test]
    fn the_archive_capped_message_keeps_its_wire_shape() {
        let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel();

        WsSink { tx: sink_tx }.archive_capped(64, 6);

        let json = serde_json::to_string(&sink_rx.try_recv().unwrap()).unwrap();
        assert!(json.contains(r#""type":"serial_debug_archive_capped""#));
        // snake_case like every other ServerMessage field (cf. `request_id`);
        // `ws-transport.ts` maps it to `limitMib` at the boundary.
        assert!(json.contains(r#""limit_mib":64"#), "{json}");
        // The sentinel sits at line 7, so it is inside a backfill snapshot iff
        // that snapshot is >= 7, i.e. iff `archived_before` (6) < snapshot.
        assert!(json.contains(r#""archived_before":6"#), "{json}");
    }

    /// End to end through [`WsSink`]: the archive cut and the wire message have
    /// to line up, because the number in the message is what tells the frontend
    /// where the gap goes.
    #[test]
    fn dropping_device_output_cuts_the_line_and_announces_the_gap_on_the_wire() {
        let dir = std::env::temp_dir().join(format!(
            "tyutool-serve-drop-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = SerialDebugChunkBatchBuffer::new();

        // A line still being received when the queue overflowed.
        archive
            .lock()
            .unwrap()
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"before-gap".to_vec(),
            })
            .unwrap();

        serial_debug_report_drops(
            &WsSink { tx: sink_tx },
            &archive,
            &filters,
            &mut pending,
            SerialDebugDropReport {
                chunks: 3,
                bytes: 12_288,
            },
        );

        let json = serde_json::to_string(&sink_rx.try_recv().unwrap()).unwrap();
        assert!(
            json.contains(r#""type":"serial_debug_chunks_dropped""#),
            "{json}"
        );
        // snake_case on the wire; `ws-transport.ts` maps it to `droppedBytes`.
        assert!(json.contains(r#""dropped_bytes":12288"#), "{json}");
        // Nothing was archived before the gap ("before-gap" has no newline yet,
        // so it is still buffered), and `append_gap` writes both of its lines
        // under one lock — one number covers the cut line and the notice.
        assert!(json.contains(r#""archived_before":0"#), "{json}");

        // Bytes arriving after the gap must start their own line.
        let after = archive
            .lock()
            .unwrap()
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"after-gap\n".to_vec(),
            })
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].text, "after-gap");

        let lines = archive.lock().unwrap().read_line_range(1, 10).unwrap();
        let texts = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "before-gap",
                tyutool_core::serial_debug_chunk_drop_sentinel(12_288).as_str(),
                "after-gap",
            ]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn serialize_serial_debug_state_info_omits_cfg_when_closed() {
        let closed = serde_json::to_string(&ServerMessage::SerialDebugStateInfo {
            open: false,
            cfg: None,
        })
        .unwrap();
        assert!(closed.contains(r#""type":"serial_debug_state_info""#));
        assert!(closed.contains(r#""open":false"#));
        // skip_serializing_if = Option::is_none → cfg must be absent.
        assert!(
            !closed.contains("cfg"),
            "cfg should be omitted when None: {closed}"
        );

        let open = serde_json::to_string(&ServerMessage::SerialDebugStateInfo {
            open: true,
            cfg: Some(DebugConfig {
                port: "/dev/ttyUSB0".into(),
                baud_rate: 115200,
                data_bits: tyutool_core::DataBits::Eight,
                parity: tyutool_core::Parity::None,
                stop_bits: tyutool_core::StopBits::One,
            }),
        })
        .unwrap();
        assert!(open.contains(r#""open":true"#));
        assert!(
            open.contains(r#""baudRate":115200"#),
            "cfg should be present: {open}"
        );
    }

    #[test]
    fn serialize_error_and_chunk_messages() {
        let err = serde_json::to_string(&ServerMessage::Error {
            message: "boom".into(),
            request_id: None,
        })
        .unwrap();
        assert!(err.contains(r#""type":"error""#));
        assert!(err.contains(r#""message":"boom""#));

        let chunk = serde_json::to_string(&ServerMessage::SerialDebugChunk {
            chunk: ArchivedChunk {
                chunk: DebugChunk {
                    direction: tyutool_core::Direction::Tx,
                    ts_ms: 42,
                    bytes: vec![7, 8],
                },
                archived_before: 9,
            },
        })
        .unwrap();
        assert!(chunk.contains(r#""type":"serial_debug_chunk""#));
        assert!(chunk.contains(r#""direction":"tx""#));
        // Flattened into the chunk object and camelCase like `tsMs`, so
        // `ws-transport.ts` needs no mapping for it.
        assert!(chunk.contains(r#""archivedBefore":9"#), "{chunk}");

        let batch = serde_json::to_string(&ServerMessage::SerialDebugChunkBatch {
            chunks: vec![
                ArchivedChunk {
                    chunk: DebugChunk {
                        direction: tyutool_core::Direction::Rx,
                        ts_ms: 1,
                        bytes: vec![1],
                    },
                    archived_before: 0,
                },
                ArchivedChunk {
                    chunk: DebugChunk {
                        direction: tyutool_core::Direction::Rx,
                        ts_ms: 2,
                        bytes: vec![2, 3],
                    },
                    archived_before: 1,
                },
            ],
        })
        .unwrap();
        assert!(batch.contains(r#""type":"serial_debug_chunk_batch""#));
        assert!(batch.contains(r#""tsMs":2"#));
        assert!(batch.contains(r#""archivedBefore":1"#), "{batch}");
    }

    /// The frontend's mid-session auto-save handoff is exact only if every chunk
    /// reports the archive line count that existed *before* it was appended —
    /// per chunk, so a batch of several chunks carries several numbers.
    #[test]
    fn chunk_batch_reports_the_archive_position_of_each_chunk() {
        let dir = std::env::temp_dir().join(format!(
            "tyutool-serve-chunk-positions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel();

        let chunk = |bytes: &[u8]| DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1,
            bytes: bytes.to_vec(),
        };
        serial_debug_flush_chunks(
            &WsSink { tx: sink_tx },
            &archive,
            &filters,
            vec![
                chunk(b"one\n"),       // archives line 1
                chunk(b"partial"),     // archives nothing (no newline yet)
                chunk(b"-end\ntwo\n"), // archives lines 2 and 3
            ],
        );

        let positions = match sink_rx.try_recv().unwrap() {
            ServerMessage::SerialDebugChunkBatch { chunks } => {
                chunks.iter().map(|c| c.archived_before).collect::<Vec<_>>()
            }
            other => panic!("expected a chunk batch, got {other:?}"),
        };
        assert_eq!(positions, vec![0, 1, 1]);
        assert_eq!(archive.lock().unwrap().total_lines(), 3);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chunk_bridge_reset_discards_stale_partial_line_before_clear() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "tyutool-serial-debug-bridge-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (sink_tx, _sink_rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = serial_debug_spawn_chunk_bridge(
            WsSink { tx: sink_tx },
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );

        bridge.send_chunk(DebugChunk {
            direction: tyutool_core::Direction::Rx,
            ts_ms: 1,
            bytes: b"pre".to_vec(),
        });
        bridge.reset().unwrap();
        archive.lock().unwrap().clear().unwrap();

        bridge.send_chunk(DebugChunk {
            direction: tyutool_core::Direction::Rx,
            ts_ms: 2,
            bytes: b"post\nnew\n".to_vec(),
        });
        bridge.shutdown().unwrap();

        let page = archive.lock().unwrap().read_page(0, 10).unwrap();
        assert_eq!(page.total_lines, 2);
        assert_eq!(
            page.items
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["post", "new"]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    // ── 消息循环的响应性 ──────────────────────────────────────────────────
    //
    // 这里驱动的是真实的 WS 服务端（run_serve）和真实的 run_job，只是芯片换成了
    // tyutool-core 的 MOCK 假设备（见本 crate 的 dev-dependency）。全程不需要硬件。
    //
    // 验的不是协议，而是一件单看代码很难发现、手点更难发现的事：任务跑起来之后，
    // 这条连接是否还接得进下一条客户端消息。
    mod loop_responsiveness {
        use futures_util::{SinkExt, StreamExt};
        use tokio::net::TcpStream;
        use tokio::time::{timeout, Duration};
        use tokio_tungstenite::tungstenite::Message;
        use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

        type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

        /// MOCK 芯片的模拟烧录约 1.5 秒，这里留足余量；帧一直不来时也能快速失败。
        const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

        /// Asks the OS for a free port and lets go of it again. A tiny race with
        /// another process remains; a fixed port would instead collide with a
        /// developer running `tyutool-cli serve`, which is worse.
        async fn start_server() -> u16 {
            let port = {
                let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("binding a loopback port");
                probe.local_addr().expect("probe has an address").port()
            };
            tokio::spawn(super::super::run_serve(port));

            for _ in 0..100 {
                if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                    return port;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("run_serve never started listening on port {port}");
        }

        async fn connect(port: u16) -> Ws {
            let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
                .await
                .expect("dev-serve should accept a loopback client sending no Origin header");
            ws
        }

        async fn send(ws: &mut Ws, frame: serde_json::Value) {
            ws.send(Message::Text(frame.to_string().into()))
                .await
                .expect("client frame should reach the server");
        }

        /// Reads until a `progress` frame carrying `kind` arrives and returns its
        /// payload. Anything else on the wire is skipped.
        async fn read_progress(ws: &mut Ws, kind: &str) -> serde_json::Value {
            let found = timeout(REPLY_TIMEOUT, async {
                while let Some(Ok(msg)) = ws.next().await {
                    let Message::Text(text) = msg else { continue };
                    let Ok(frame) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                        continue;
                    };
                    if frame.get("type").and_then(|v| v.as_str()) != Some("progress") {
                        continue;
                    }
                    let payload = frame.get("payload").cloned().unwrap_or_default();
                    if payload.get("kind").and_then(|v| v.as_str()) == Some(kind) {
                        return Some(payload);
                    }
                }
                None
            })
            .await;

            match found {
                Ok(Some(payload)) => payload,
                Ok(None) => panic!("the connection closed before any {kind} frame arrived"),
                Err(_) => panic!("no {kind} frame within {REPLY_TIMEOUT:?}"),
            }
        }

        /// Like [`read_progress`], but for an `error` frame; returns its message.
        async fn read_error(ws: &mut Ws) -> String {
            let found = timeout(REPLY_TIMEOUT, async {
                while let Some(Ok(msg)) = ws.next().await {
                    let Message::Text(text) = msg else { continue };
                    let Ok(frame) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                        continue;
                    };
                    if frame.get("type").and_then(|v| v.as_str()) == Some("error") {
                        return frame
                            .get("message")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                }
                None
            })
            .await;

            match found {
                Ok(Some(message)) => message,
                Ok(None) => panic!("the connection closed before any error frame arrived"),
                Err(_) => panic!("no error frame within {REPLY_TIMEOUT:?}"),
            }
        }

        fn mock_job() -> serde_json::Value {
            serde_json::json!({
                "type": "run_job",
                "job": {
                    "mode": "flash",
                    "chipId": "MOCK",
                    "port": "/dev/mock",
                    "baudRate": 115200
                }
            })
        }

        /// 基线：一个 MOCK 任务确实能通过 WS 从头跑到尾。
        #[tokio::test]
        async fn a_mock_job_runs_to_completion_over_the_socket() {
            let port = start_server().await;
            let mut ws = connect(port).await;

            send(&mut ws, mock_job()).await;
            let done = read_progress(&mut ws, "done").await;

            assert!(
                done.pointer("/result/ok").is_some(),
                "a job left alone should finish Ok; got {done}",
            );
        }

        /// 任务运行期间发出的 cancel，必须被读到并生效。
        #[tokio::test]
        async fn cancel_sent_while_a_job_runs_is_acted_on() {
            let port = start_server().await;
            let mut ws = connect(port).await;

            send(&mut ws, mock_job()).await;
            // 等到任务确实在跑，免得取消只是赢在了任务开始之前。
            read_progress(&mut ws, "percent").await;

            send(&mut ws, serde_json::json!({ "type": "cancel" })).await;

            let done = read_progress(&mut ws, "done").await;
            assert!(
                done.pointer("/result/cancelled").is_some(),
                "cancel was sent mid-job, but the run ended as {done} instead of cancelled \
                 — the connection stopped reading client frames while the job held the loop",
            );
        }

        /// 一条连接上同时只跑一个任务。以前这是靠「循环被阻住」隐含保证的，现在
        /// 循环不再阻塞，就得明确拒绝——否则第二个任务会把共享的取消标志清掉，
        /// 把第一个任务的取消一并取消了。
        #[tokio::test]
        async fn a_second_job_on_the_same_connection_is_refused() {
            let port = start_server().await;
            let mut ws = connect(port).await;

            send(&mut ws, mock_job()).await;
            read_progress(&mut ws, "percent").await;
            send(&mut ws, mock_job()).await;

            let message = read_error(&mut ws).await;
            assert!(
                message.contains("already running"),
                "a second job should be refused while one is in flight; got {message:?}",
            );
        }
    }
}
