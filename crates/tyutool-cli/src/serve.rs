//! WebSocket dev-serve mode for tyutool-cli.
//! Exposes serial port operations over a local WebSocket so the Vite dev
//! server (localhost:1420) can flash real devices without the Tauri shell.
//!
//! Usage: tyutool-cli serve [--port 9527]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tyutool_core::{
    device_reset_dtr_rts, list_serial_ports, run_job, serial_debug_fail_backfill_if_current,
    serial_debug_finish_backfill_if_current, serial_debug_scan_filter_matches, DebugChunk,
    DebugConfig, FlashJob, SerialDebugArchive, SerialDebugArchiveReader,
    SerialDebugChunkBatchBuffer, SerialDebugFilterBackfillSnapshot, SerialDebugFilterDefinition,
    SerialDebugFilterIndex, SerialDebugFilterPage, SerialDebugFilterStats, SerialDebugGeneration,
    SerialDebugLine, SerialDebugSession, SerialDebugSessionPage, SerialPortEntry,
};

const SERIAL_DEBUG_CHUNK_FLUSH_MS: u64 = 12;
const SERIAL_DEBUG_CHUNK_FLUSH_BYTES: usize = 32 * 1024;
const SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY: usize = 256;

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
    Progress {
        payload: serde_json::Value,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SerialDebugChunk {
        chunk: DebugChunk,
    },
    SerialDebugChunkBatch {
        chunks: Vec<DebugChunk>,
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
}

enum SerialDebugChunkBridgeMessage {
    Chunk {
        generation: u64,
        chunk: DebugChunk,
    },
    Reset {
        generation: u64,
        ack: std::sync::mpsc::SyncSender<()>,
    },
}

#[derive(Clone)]
struct SerialDebugChunkBridgeHandle {
    generation: Arc<SerialDebugGeneration>,
    send_lock: Arc<Mutex<()>>,
    tx: SyncSender<SerialDebugChunkBridgeMessage>,
}

impl SerialDebugChunkBridgeHandle {
    fn send_chunk(
        &self,
        chunk: DebugChunk,
    ) -> Result<(), std::sync::mpsc::SendError<SerialDebugChunkBridgeMessage>> {
        let _guard = self.send_lock.lock().unwrap();
        self.tx.send(SerialDebugChunkBridgeMessage::Chunk {
            generation: self.generation.current(),
            chunk,
        })
    }

    fn reset(&self) -> Result<u64, String> {
        let _guard = self.send_lock.lock().unwrap();
        let generation = self.generation.advance();
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        self.tx
            .send(SerialDebugChunkBridgeMessage::Reset {
                generation,
                ack: ack_tx,
            })
            .map_err(|e| e.to_string())?;
        ack_rx.recv().map_err(|e| e.to_string())?;
        Ok(generation)
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

// ── Per-connection handler ───────────────────────────────────────────────────

async fn handle_connection(stream: tokio::net::TcpStream) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
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

    // ── mpsc sink pump ───────────────────────────────────────────────────────
    // Background tasks (progress callbacks, serial-debug reader thread) need to
    // push ServerMessage values from contexts that don't own the WS sink. Wrap
    // the sink in a single drainer task fed by an unbounded mpsc.
    use tokio::sync::mpsc;
    let (sink_tx, mut sink_rx) = mpsc::unbounded_channel::<ServerMessage>();

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
            if sink_moved.send(Message::Text(text)).await.is_err() {
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
    let debug_archive = Arc::new(Mutex::new(
        SerialDebugArchive::create(&serial_debug_dir).expect("create serial-debug archive"),
    ));
    let debug_filters = Arc::new(Mutex::new(
        SerialDebugFilterIndex::create(&serial_debug_dir).expect("create serial-debug filters"),
    ));

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
                mut job,
                file_content,
                file_contents,
            } => {
                cancel.store(false, Ordering::Relaxed);
                handle_run_job(
                    &sink_tx,
                    Arc::clone(&cancel),
                    &mut job,
                    file_content,
                    file_contents,
                    Arc::clone(&pending_confirm),
                )
                .await;
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
                let sink_for_chunk = sink_tx.clone();
                let sink_for_disc = sink_tx.clone();
                let archive_for_chunk = Arc::clone(&debug_archive);
                let filters_for_chunk = Arc::clone(&debug_filters);
                let chunk_bridge = spawn_serial_debug_chunk_bridge_ws(
                    sink_for_chunk.clone(),
                    Arc::clone(&archive_for_chunk),
                    Arc::clone(&filters_for_chunk),
                    Arc::clone(&debug_generation),
                );
                let chunk_bridge_for_session = chunk_bridge.clone();
                let result = SerialDebugSession::open(
                    cfg,
                    Box::new(move |chunk| {
                        let _ = chunk_bridge_for_session.send_chunk(chunk);
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
                debug_chunk_bridge.lock().unwrap().take();
                let _ = sink_tx.send(ServerMessage::SerialDebugClosed);
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
                    let completed = {
                        let mut archive = debug_archive.lock().unwrap();
                        archive.append_chunk(&chunk).unwrap_or_default()
                    };
                    ingest_serial_debug_lines_ws(&sink_tx, &debug_filters, &completed);
                    let _ = sink_tx.send(ServerMessage::SerialDebugChunk { chunk });
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
                ingest_serial_debug_lines_ws(&sink_tx, &debug_filters, &[line]);
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

    // Clean up any open serial-debug session before dropping the sink.
    if let Ok(mut guard) = debug_session.lock() {
        if let Some(s) = guard.take() {
            s.close();
        }
    }
    if let Ok(mut guard) = debug_chunk_bridge.lock() {
        guard.take();
    }

    drop(sink_tx);
    let _ = pump.await;

    log::info!("WS connection closed");
}

fn flush_serial_debug_chunk_ws(
    sink_tx: &tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    archive: &Arc<Mutex<SerialDebugArchive>>,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
    chunks: Vec<DebugChunk>,
) {
    if chunks.is_empty() {
        return;
    }
    let completed = {
        let mut archive = match archive.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let mut completed = Vec::new();
        for chunk in &chunks {
            completed.extend(archive.append_chunk(chunk).unwrap_or_default());
        }
        completed
    };
    ingest_serial_debug_lines_ws(sink_tx, filters, &completed);
    let _ = sink_tx.send(ServerMessage::SerialDebugChunkBatch { chunks });
}

fn spawn_serial_debug_chunk_bridge_ws(
    sink_tx: tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    archive: Arc<Mutex<SerialDebugArchive>>,
    filters: Arc<Mutex<SerialDebugFilterIndex>>,
    generation: Arc<SerialDebugGeneration>,
) -> SerialDebugChunkBridgeHandle {
    // Bound the bridge queue so sustained ingress can't grow process memory without limit
    // when archive/filter/UI consumption temporarily lags behind the serial reader.
    let (tx, rx) =
        mpsc::sync_channel::<SerialDebugChunkBridgeMessage>(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY);
    let handle = SerialDebugChunkBridgeHandle {
        generation: Arc::clone(&generation),
        send_lock: Arc::new(Mutex::new(())),
        tx: tx.clone(),
    };
    std::thread::spawn(move || {
        let mut pending = SerialDebugChunkBatchBuffer::new();
        let mut active_generation = generation.current();
        loop {
            match rx.recv_timeout(Duration::from_millis(SERIAL_DEBUG_CHUNK_FLUSH_MS)) {
                Ok(SerialDebugChunkBridgeMessage::Chunk { generation, chunk }) => {
                    if generation != active_generation {
                        if generation < active_generation {
                            continue;
                        }
                        let _ = pending.take();
                        active_generation = generation;
                    }
                    pending.push(chunk);
                    if pending.should_flush_bytes(SERIAL_DEBUG_CHUNK_FLUSH_BYTES) {
                        flush_serial_debug_chunk_ws(&sink_tx, &archive, &filters, pending.take());
                    }
                }
                Ok(SerialDebugChunkBridgeMessage::Reset { generation, ack }) => {
                    let _ = pending.take();
                    active_generation = generation;
                    let _ = ack.send(());
                }
                Err(RecvTimeoutError::Timeout) => {
                    if pending
                        .should_flush_elapsed(Duration::from_millis(SERIAL_DEBUG_CHUNK_FLUSH_MS))
                    {
                        flush_serial_debug_chunk_ws(&sink_tx, &archive, &filters, pending.take());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    flush_serial_debug_chunk_ws(&sink_tx, &archive, &filters, pending.take());
                    return;
                }
            }
        }
    });
    handle
}

fn ingest_serial_debug_lines_ws(
    sink_tx: &tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
    lines: &[SerialDebugLine],
) {
    if lines.is_empty() {
        return;
    }
    let updates = {
        let mut guard = match filters.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.ingest_completed_lines(lines).unwrap_or_default()
    };
    if updates.is_empty() {
        return;
    }
    let guard = match filters.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    for stats in updates {
        if let Some(def) = guard.definition(&stats.filter_id) {
            let _ = sink_tx.send(ServerMessage::SerialDebugFilterUpdated {
                def,
                stats,
                request_id: None,
            });
        }
    }
}

// ── Run job handler ──────────────────────────────────────────────────────────

async fn handle_run_job(
    sink_tx: &tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    cancel: Arc<AtomicBool>,
    job: &mut FlashJob,
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
            chunk: DebugChunk {
                direction: tyutool_core::Direction::Tx,
                ts_ms: 42,
                bytes: vec![7, 8],
            },
        })
        .unwrap();
        assert!(chunk.contains(r#""type":"serial_debug_chunk""#));
        assert!(chunk.contains(r#""direction":"tx""#));

        let batch = serde_json::to_string(&ServerMessage::SerialDebugChunkBatch {
            chunks: vec![
                DebugChunk {
                    direction: tyutool_core::Direction::Rx,
                    ts_ms: 1,
                    bytes: vec![1],
                },
                DebugChunk {
                    direction: tyutool_core::Direction::Rx,
                    ts_ms: 2,
                    bytes: vec![2, 3],
                },
            ],
        })
        .unwrap();
        assert!(batch.contains(r#""type":"serial_debug_chunk_batch""#));
        assert!(batch.contains(r#""tsMs":2"#));
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
        let bridge = spawn_serial_debug_chunk_bridge_ws(
            sink_tx,
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );

        bridge
            .send_chunk(DebugChunk {
                direction: tyutool_core::Direction::Rx,
                ts_ms: 1,
                bytes: b"pre".to_vec(),
            })
            .unwrap();
        bridge.reset().unwrap();
        archive.lock().unwrap().clear().unwrap();

        bridge
            .send_chunk(DebugChunk {
                direction: tyutool_core::Direction::Rx,
                ts_ms: 2,
                bytes: b"post\nnew\n".to_vec(),
            })
            .unwrap();
        drop(bridge);
        std::thread::sleep(Duration::from_millis(50));

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
}
