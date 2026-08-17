//! Serial-debug session: open/close, send, the chunk bridge that batches device
//! output for the UI, filters, and device reset.
//!
//! The chunk bridge is why this is more than a thin command wrapper: device
//! output arrives line-by-line but reaches the webview in coalesced chunks
//! (`SERIAL_DEBUG_CHUNK_FLUSH_*`) so a chatty device cannot flood the IPC
//! channel.

use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use tyutool_core::{
    serial_debug_fail_backfill_if_current, serial_debug_finish_backfill_if_current,
    serial_debug_scan_filter_matches, DebugChunk, DebugConfig, SerialDebugArchive,
    SerialDebugArchiveReader, SerialDebugChunkBatchBuffer, SerialDebugFilterBackfillSnapshot,
    SerialDebugFilterDefinition, SerialDebugFilterIndex, SerialDebugFilterPage,
    SerialDebugFilterStats, SerialDebugGeneration, SerialDebugLine, SerialDebugSession,
    SerialDebugSessionPage,
};

pub(crate) struct DebugState {
    pub session: Arc<StdMutex<Option<SerialDebugSession>>>,
    pub archive: Arc<StdMutex<SerialDebugArchive>>,
    pub filters: Arc<StdMutex<SerialDebugFilterIndex>>,
    pub chunk_bridge: Arc<StdMutex<Option<SerialDebugChunkBridgeHandle>>>,
    pub generation: Arc<SerialDebugGeneration>,
}

#[derive(Clone, serde::Serialize)]
struct DisconnectPayload {
    reason: String,
}

/// Payload of `serial-debug-archive-capped`. The frontend renders the notice
/// itself from `serialDebug.log.archiveCapped`, so only the number crosses.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveCappedPayload {
    limit_mib: u64,
}

#[derive(Clone, Serialize)]
pub(crate) struct SerialDebugFilterUpdatePayload {
    def: SerialDebugFilterDefinition,
    stats: SerialDebugFilterStats,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SerialDebugFilterAddArgs {
    keyword: String,
    use_regex: bool,
    color: String,
}

const SERIAL_DEBUG_CHUNK_FLUSH_MS: u64 = 12;
const SERIAL_DEBUG_CHUNK_FLUSH_BYTES: usize = 32 * 1024;
const SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY: usize = 256;

fn serial_debug_archive_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("tyutool").join("serial-debug")
}

/// Create the serial-debug archive + filter index without panicking the GUI on
/// startup. The preferred directory is `serial_debug_archive_dir()`; if that is
/// not writable (permissions, a stale lock, antivirus interference) we fall back
/// to a per-process-unique subdirectory and log a warning. Only if every attempt
/// fails do we propagate the error — at which point the app genuinely cannot
/// function and a controlled panic with a clear message is preferable to a
/// silent half-initialised state.
pub(crate) fn create_serial_debug_archive_resilient() -> (SerialDebugArchive, SerialDebugFilterIndex)
{
    let primary = serial_debug_archive_dir();
    match (
        SerialDebugArchive::create(&primary),
        SerialDebugFilterIndex::create(&primary),
    ) {
        (Ok(a), Ok(f)) => return (a, f),
        (a_res, f_res) => {
            log::warn!(
                "[serial-debug] archive dir {:?} unavailable \
                 (archive={:?}, filters={:?}); retrying in a per-process dir",
                primary,
                a_res.err().map(|e| e.to_string()),
                f_res.err().map(|e| e.to_string()),
            );
        }
    }
    // Per-process fallback so a stale/locked primary dir doesn't block startup.
    let fallback = serial_debug_archive_dir().join(format!("pid-{}", std::process::id()));
    match (
        SerialDebugArchive::create(&fallback),
        SerialDebugFilterIndex::create(&fallback),
    ) {
        (Ok(a), Ok(f)) => {
            log::warn!(
                "[serial-debug] archive initialised in fallback dir {:?} \
                 (serial-debug persistence may be split across dirs)",
                fallback
            );
            (a, f)
        }
        (a_res, f_res) => {
            panic!(
                "serial-debug archive could not be created in {:?} or {:?}: \
                 archive={:?}, filters={:?}",
                primary,
                fallback,
                a_res.err().map(|e| e.to_string()),
                f_res.err().map(|e| e.to_string()),
            );
        }
    }
}

fn emit_filter_update(
    app: &AppHandle,
    def: &SerialDebugFilterDefinition,
    stats: &SerialDebugFilterStats,
) {
    let _ = app.emit(
        "serial-debug-filter-updated",
        &SerialDebugFilterUpdatePayload {
            def: def.clone(),
            stats: stats.clone(),
        },
    );
}

/// Every line the archive accepts passes through `ingest_serial_debug_lines`,
/// which makes it the one place that can spot the archive-cap sentinel. The
/// live view never sees archived lines — it re-splits the raw
/// `serial-debug-chunk*` payloads itself — so without this event the cap notice
/// would only ever exist in the archive file and the user would watch the log
/// keep scrolling with no hint that recording had stopped.
fn emit_archive_cap_notice(app: &AppHandle, lines: &[SerialDebugLine]) {
    if let Some(limit_mib) = lines
        .iter()
        .find_map(tyutool_core::serial_debug_archive_cap_limit_mib)
    {
        let _ = app.emit(
            "serial-debug-archive-capped",
            &ArchiveCappedPayload { limit_mib },
        );
    }
}

fn ingest_serial_debug_lines(
    app: &AppHandle,
    archive: &Arc<StdMutex<SerialDebugArchive>>,
    filters: &Arc<StdMutex<SerialDebugFilterIndex>>,
    lines: &[SerialDebugLine],
) {
    if lines.is_empty() {
        return;
    }
    emit_archive_cap_notice(app, lines);
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
            emit_filter_update(app, &def, &stats);
        }
    }
    let _ = archive; // keeps the signature symmetric with other bridge helpers
}

fn flush_serial_debug_chunk(
    app: &AppHandle,
    archive: &Arc<StdMutex<SerialDebugArchive>>,
    filters: &Arc<StdMutex<SerialDebugFilterIndex>>,
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
    ingest_serial_debug_lines(app, archive, filters, &completed);
    let _ = app.emit("serial-debug-chunk-batch", &chunks);
}

enum SerialDebugChunkBridgeMessage {
    Chunk {
        generation: u64,
        chunk: DebugChunk,
    },
    Reset {
        generation: u64,
        ack: SyncSender<()>,
    },
}

#[derive(Clone)]
pub(crate) struct SerialDebugChunkBridgeHandle {
    generation: Arc<SerialDebugGeneration>,
    send_lock: Arc<StdMutex<()>>,
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

fn spawn_serial_debug_chunk_bridge(
    app: AppHandle,
    archive: Arc<StdMutex<SerialDebugArchive>>,
    filters: Arc<StdMutex<SerialDebugFilterIndex>>,
    generation: Arc<SerialDebugGeneration>,
) -> SerialDebugChunkBridgeHandle {
    // Bound the bridge queue so sustained ingress can't grow process memory without limit
    // when archive/filter/UI consumption temporarily lags behind the serial reader.
    let (tx, rx) =
        mpsc::sync_channel::<SerialDebugChunkBridgeMessage>(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY);
    let handle = SerialDebugChunkBridgeHandle {
        generation: Arc::clone(&generation),
        send_lock: Arc::new(StdMutex::new(())),
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
                        flush_serial_debug_chunk(&app, &archive, &filters, pending.take());
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
                        flush_serial_debug_chunk(&app, &archive, &filters, pending.take());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    flush_serial_debug_chunk(&app, &archive, &filters, pending.take());
                    return;
                }
            }
        }
    });
    handle
}

#[tauri::command]
pub(crate) async fn serial_debug_open(
    app: AppHandle,
    state: State<'_, DebugState>,
    cfg: DebugConfig,
) -> Result<(), String> {
    log::info!(
        "[serial-debug] open port={} baud={}",
        cfg.port,
        cfg.baud_rate
    );
    {
        let guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        if guard.is_some() {
            log::warn!("[serial-debug] open rejected: already open");
            return Err("already open".into());
        }
    }
    let archive_for_chunk = Arc::clone(&state.archive);
    let filters_for_chunk = Arc::clone(&state.filters);
    let app_for_chunk = app.clone();
    let app_for_disc = app.clone();
    let chunk_tx = spawn_serial_debug_chunk_bridge(
        app_for_chunk.clone(),
        Arc::clone(&archive_for_chunk),
        Arc::clone(&filters_for_chunk),
        Arc::clone(&state.generation),
    );
    let chunk_tx_for_session = chunk_tx.clone();
    // Run the blocking serialport::open() off the main thread.
    let session = tauri::async_runtime::spawn_blocking(move || {
        SerialDebugSession::open(
            cfg,
            Box::new(move |chunk: DebugChunk| {
                let _ = chunk_tx_for_session.send_chunk(chunk);
            }),
            Box::new(move |reason: String| {
                let _ =
                    app_for_disc.emit("serial-debug-disconnected", &DisconnectPayload { reason });
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let mut guard = state
        .session
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?;
    if guard.is_some() {
        // Another open won the race while we were in spawn_blocking; discard this session.
        log::warn!("[serial-debug] open lost race: already open");
        session.close();
        return Err("already open".into());
    }
    *guard = Some(session);
    *state
        .chunk_bridge
        .lock()
        .map_err(|_| "debug state poisoned".to_string())? = Some(chunk_tx);
    log::info!("[serial-debug] open ok");
    Ok(())
}

#[tauri::command]
pub(crate) async fn serial_debug_close(state: State<'_, DebugState>) -> Result<(), String> {
    let session = {
        let mut guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        guard.take()
    };
    if let Some(session) = session {
        log::info!("[serial-debug] close");
        // h.join() blocks; run it off the async runtime thread.
        tauri::async_runtime::spawn_blocking(move || session.close())
            .await
            .map_err(|e| e.to_string())?;
    }
    state
        .chunk_bridge
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?
        .take();
    Ok(())
}

#[tauri::command]
pub(crate) fn serial_debug_send(
    app: AppHandle,
    state: State<'_, DebugState>,
    bytes: Vec<u8>,
) -> Result<(), String> {
    log::debug!("[serial-debug] send {} bytes", bytes.len());
    {
        let guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        let session = guard.as_ref().ok_or_else(|| {
            log::warn!("[serial-debug] send rejected: not open");
            "serial debug not open".to_string()
        })?;
        session.write(&bytes).map_err(|e| e.to_string())?;
    } // DebugState lock dropped here — emit happens unlocked
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let chunk = tyutool_core::DebugChunk {
        direction: tyutool_core::Direction::Tx,
        ts_ms,
        bytes: bytes.clone(),
    };
    let completed = {
        let mut archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        archive
            .append_chunk(&chunk)
            .map_err(|e| format!("append serial debug tx chunk failed: {e}"))?
    };
    ingest_serial_debug_lines(&app, &state.archive, &state.filters, &completed);
    let _ = app.emit("serial-debug-chunk", &chunk);
    Ok(())
}

#[tauri::command]
pub(crate) fn serial_debug_state(
    state: State<'_, DebugState>,
) -> Result<Option<DebugConfig>, String> {
    let guard = state
        .session
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?;
    Ok(guard.as_ref().map(|s| s.config().clone()))
}

#[tauri::command]
pub(crate) fn serial_debug_session_clear(
    app: AppHandle,
    state: State<'_, DebugState>,
) -> Result<(), String> {
    let chunk_bridge = state
        .chunk_bridge
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?
        .as_ref()
        .cloned();
    if let Some(bridge) = chunk_bridge {
        bridge.reset()?;
    } else {
        state.generation.advance();
    }
    {
        let mut archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        archive.clear().map_err(|e| e.to_string())?;
    }
    let updates = {
        let mut filters = state
            .filters
            .lock()
            .map_err(|_| "debug filters poisoned".to_string())?;
        filters.reset_for_new_session()
    };
    let filters = state
        .filters
        .lock()
        .map_err(|_| "debug filters poisoned".to_string())?;
    for stats in updates {
        if let Some(def) = filters.definition(&stats.filter_id) {
            emit_filter_update(&app, &def, &stats);
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn serial_debug_append_sys_line(
    app: AppHandle,
    state: State<'_, DebugState>,
    ts_ms: u64,
    text: String,
) -> Result<(), String> {
    let line = {
        let mut archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        archive
            .append_sys_line(ts_ms, text)
            .map_err(|e| format!("append sys line failed: {e}"))?
    };
    // `None` once the session archive hit its size cap.
    if let Some(line) = line {
        ingest_serial_debug_lines(&app, &state.archive, &state.filters, &[line]);
    }
    Ok(())
}

/// Push the user's archive-size limit (MiB in the UI, bytes on the wire) down to
/// the archive. Mirrors the `set_log_level` shape: a setting the frontend owns
/// and re-applies on load and on change.
#[tauri::command]
pub(crate) fn serial_debug_set_archive_limit(
    state: State<'_, DebugState>,
    max_bytes: u64,
) -> Result<(), String> {
    state
        .archive
        .lock()
        .map_err(|_| "debug archive poisoned".to_string())?
        .set_max_bytes(max_bytes);
    Ok(())
}

#[tauri::command]
pub(crate) fn serial_debug_filter_add(
    app: AppHandle,
    state: State<'_, DebugState>,
    args: SerialDebugFilterAddArgs,
) -> Result<SerialDebugFilterUpdatePayload, String> {
    let snapshot_total_lines = {
        let archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        archive.total_lines()
    };
    let current_generation = state.generation.current();
    let def = {
        let mut filters = state
            .filters
            .lock()
            .map_err(|_| "debug filters poisoned".to_string())?;
        filters.add_filter(
            args.keyword,
            args.use_regex,
            args.color,
            snapshot_total_lines,
        )?
    };
    let initial = {
        let filters = state
            .filters
            .lock()
            .map_err(|_| "debug filters poisoned".to_string())?;
        filters
            .stats(&def.id)
            .ok_or_else(|| "new filter stats missing".to_string())?
    };
    let (backfill_stats, backfill_snapshot, archive_reader): (
        SerialDebugFilterStats,
        SerialDebugFilterBackfillSnapshot,
        SerialDebugArchiveReader,
    ) = {
        let mut filters = state
            .filters
            .lock()
            .map_err(|_| "debug filters poisoned".to_string())?;
        let stats = filters.start_backfill(&def.id).map_err(|e| e.to_string())?;
        let snapshot = filters
            .backfill_snapshot(&def.id)
            .ok_or_else(|| "new filter backfill snapshot missing".to_string())?;
        drop(filters);
        let archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        (stats, snapshot, archive.snapshot_reader())
    };
    emit_filter_update(&app, &def, &backfill_stats);

    let app_for_backfill = app.clone();
    let filters_for_backfill = Arc::clone(&state.filters);
    let generation_for_backfill = Arc::clone(&state.generation);
    let filter_id = def.id.clone();
    let historical_idx_path =
        serial_debug_archive_dir().join(format!("serial-debug-filter-{filter_id}.historical.idx"));
    tauri::async_runtime::spawn_blocking(move || {
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
                emit_filter_update(&app_for_backfill, &def, &stats);
            }
        }
    });

    Ok(SerialDebugFilterUpdatePayload {
        def,
        stats: initial,
    })
}

#[tauri::command]
pub(crate) fn serial_debug_filter_remove(
    state: State<'_, DebugState>,
    filter_id: String,
) -> Result<(), String> {
    let removed = state
        .filters
        .lock()
        .map_err(|_| "debug filters poisoned".to_string())?
        .remove_filter(&filter_id);
    if removed {
        Ok(())
    } else {
        Err("filter not found".into())
    }
}

#[tauri::command]
pub(crate) fn serial_debug_filter_read_matches(
    state: State<'_, DebugState>,
    filter_id: String,
    start: u64,
    limit: u64,
) -> Result<SerialDebugFilterPage, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "debug archive poisoned".to_string())?;
    let filters = state
        .filters
        .lock()
        .map_err(|_| "debug filters poisoned".to_string())?;
    filters
        .read_match_page(&filter_id, start, limit, &archive)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn serial_debug_session_read_page(
    state: State<'_, DebugState>,
    start: u64,
    limit: u64,
) -> Result<SerialDebugSessionPage, String> {
    state
        .archive
        .lock()
        .map_err(|_| "debug archive poisoned".to_string())?
        .read_page(start, limit)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceResetArgs {
    port: String,
    chip_id: String,
}

fn serial_debug_device_reset_session(
    session: Option<&SerialDebugSession>,
    chip_id: &str,
) -> Result<(), String> {
    let active = session.ok_or_else(|| "serial debug not open".to_string())?;
    active.device_reset(chip_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn device_reset_cmd(args: DeviceResetArgs) -> Result<(), String> {
    log::info!(
        "[Serial] Device reset (DTR/RTS): port={}, chip_id={}",
        args.port,
        args.chip_id
    );
    tyutool_core::device_reset_dtr_rts(&args.port, &args.chip_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn serial_debug_device_reset_cmd(
    state: State<'_, DebugState>,
    chip_id: String,
) -> Result<(), String> {
    let guard = state
        .session
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?;
    let port = guard
        .as_ref()
        .map(|session| session.config().port.clone())
        .unwrap_or_else(|| "<closed>".to_string());
    log::info!(
        "[SerialDebug] Device reset (DTR/RTS): port={}, chip_id={}",
        port,
        chip_id
    );
    serial_debug_device_reset_session(guard.as_ref(), &chip_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_debug_device_reset_session_requires_open_session() {
        let err = serial_debug_device_reset_session(None, "T5AI").unwrap_err();
        assert_eq!(err, "serial debug not open");
    }
}
