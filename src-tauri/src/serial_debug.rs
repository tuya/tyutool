//! Serial-debug session: open/close, send, filters, and device reset — the Tauri
//! host's half of the serial-debug feature.
//!
//! The chunk bridge that batches device output, bounds its queue and reports what
//! it had to drop lives in `tyutool_core::serial_debug_bridge`, shared with
//! `tyutool-serve`. All this file contributes to it is [`TauriSink`]: the four
//! Tauri events those batches and notices become.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use tyutool_core::{
    serial_debug_fail_backfill_if_current, serial_debug_finalize_pending,
    serial_debug_finish_backfill_if_current, serial_debug_ingest_lines,
    serial_debug_scan_filter_matches, serial_debug_spawn_chunk_bridge, ArchivedChunk, DebugChunk,
    DebugConfig, SerialDebugArchive, SerialDebugArchiveReader, SerialDebugChunkBridgeHandle,
    SerialDebugFilterBackfillSnapshot, SerialDebugFilterDefinition, SerialDebugFilterIndex,
    SerialDebugFilterPage, SerialDebugFilterStats, SerialDebugGeneration, SerialDebugSession,
    SerialDebugSessionPage, SerialDebugSink,
};

pub(crate) struct DebugState {
    pub session: Arc<StdMutex<Option<SerialDebugSession>>>,
    pub archive: Arc<StdMutex<SerialDebugArchive>>,
    pub filters: Arc<StdMutex<SerialDebugFilterIndex>>,
    pub chunk_bridge: Arc<StdMutex<Option<SerialDebugChunkBridgeHandle>>>,
    pub generation: Arc<SerialDebugGeneration>,
    /// Serializes open/close operations without holding a mutex across awaits.
    pub lifecycle_busy: Arc<AtomicBool>,
    /// The directory `create_serial_debug_state_resilient` actually used for
    /// `archive`/`filters` — the preferred primary, or its per-process fallback
    /// when primary was unwritable at startup. Anything that re-derives a path
    /// alongside the archive (e.g. the backfill `.historical.idx`) must join
    /// onto this field, never recompute `serial_debug_archive_dir()`, or it
    /// will target the primary even when the archive actually lives in the
    /// fallback.
    pub archive_dir: PathBuf,
}

struct SerialDebugLifecycleReservation(Arc<AtomicBool>);

impl SerialDebugLifecycleReservation {
    fn acquire(lifecycle_busy: &Arc<AtomicBool>) -> Result<Self, String> {
        lifecycle_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "serial debug lifecycle operation in progress".to_string())?;
        Ok(Self(Arc::clone(lifecycle_busy)))
    }
}

impl Drop for SerialDebugLifecycleReservation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Path of a filter's backfill scratch index, alongside the archive it was
/// built from. Pulled out so the "must be under the actual archive dir, not
/// the primary" contract is unit-testable without a Tauri app.
fn historical_idx_path(archive_dir: &Path, filter_id: &str) -> PathBuf {
    archive_dir.join(format!("serial-debug-filter-{filter_id}.historical.idx"))
}

/// Emit the desktop close acknowledgement only after core finishes its drain.
fn close_bridge_and_signal<S, F>(
    bridge: SerialDebugChunkBridgeHandle,
    archive: &Arc<StdMutex<SerialDebugArchive>>,
    filters: &Arc<StdMutex<SerialDebugFilterIndex>>,
    sink: &S,
    on_closed: F,
) -> Result<(), String>
where
    S: SerialDebugSink,
    F: FnOnce(),
{
    tyutool_core::serial_debug_shutdown_and_finalize(bridge, archive, filters, sink)?;
    on_closed();
    Ok(())
}

fn finalize_and_signal<S, F>(
    archive: &Arc<StdMutex<SerialDebugArchive>>,
    filters: &Arc<StdMutex<SerialDebugFilterIndex>>,
    sink: &S,
    on_closed: F,
) where
    S: SerialDebugSink,
    F: FnOnce(),
{
    let lines = serial_debug_finalize_pending(archive);
    serial_debug_ingest_lines(sink, filters, &lines);
    on_closed();
}

/// Clear one session while excluding concurrent open/close operations. The
/// command emits the returned filter updates after this state transition ends.
fn clear_session_state(
    state: &DebugState,
) -> Result<Vec<(SerialDebugFilterDefinition, SerialDebugFilterStats)>, String> {
    let _lifecycle = SerialDebugLifecycleReservation::acquire(&state.lifecycle_busy)?;
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
    let mut filters = state
        .filters
        .lock()
        .map_err(|_| "debug filters poisoned".to_string())?;
    Ok(filters
        .reset_for_new_session()
        .into_iter()
        .filter_map(|stats| filters.definition(&stats.filter_id).map(|def| (def, stats)))
        .collect())
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
    /// `archived_before` of the cap sentinel itself — see [`ArchivedChunk`].
    archived_before: u64,
}

/// Turns everything the shared chunk bridge produces into Tauri events.
///
/// Emits are fire-and-forget by contract ([`SerialDebugSink`]): once the webview
/// is gone there is nobody left to tell, and the bridge thread must not stall on
/// finding that out.
#[derive(Clone)]
struct TauriSink {
    app: AppHandle,
}

impl SerialDebugSink for TauriSink {
    fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
        let _ = self.app.emit("serial-debug-chunk-batch", &chunks);
    }

    fn chunks_dropped(&self, dropped_bytes: u64, archived_before: u64) {
        let _ = self.app.emit(
            "serial-debug-chunks-dropped",
            &ChunksDroppedPayload {
                dropped_bytes,
                archived_before,
            },
        );
    }

    fn archive_capped(&self, limit_mib: u64, archived_before: u64) {
        let _ = self.app.emit(
            "serial-debug-archive-capped",
            &ArchiveCappedPayload {
                limit_mib,
                archived_before,
            },
        );
    }

    fn filter_updated(&self, def: SerialDebugFilterDefinition, stats: SerialDebugFilterStats) {
        emit_filter_update(&self.app, &def, &stats);
    }
}

/// Payload of `serial-debug-chunks-dropped`. Only the byte count crosses — the
/// wording comes from `serialDebug.log.chunksDropped`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunksDroppedPayload {
    dropped_bytes: u64,
    /// `archived_before` of the gap lines this notice belongs to — see
    /// [`ArchivedChunk`].
    archived_before: u64,
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

#[tauri::command]
pub(crate) async fn serial_debug_open(
    app: AppHandle,
    state: State<'_, DebugState>,
    cfg: DebugConfig,
) -> Result<(), String> {
    let _lifecycle = SerialDebugLifecycleReservation::acquire(&state.lifecycle_busy)?;
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
    let app_for_disc = app.clone();
    let chunk_tx = serial_debug_spawn_chunk_bridge(
        TauriSink { app: app.clone() },
        Arc::clone(&state.archive),
        Arc::clone(&state.filters),
        Arc::clone(&state.generation),
    );
    let chunk_tx_for_session = chunk_tx.clone();
    // Run the blocking serialport::open() off the main thread.
    let session = tauri::async_runtime::spawn_blocking(move || {
        SerialDebugSession::open(
            cfg,
            Box::new(move |chunk: DebugChunk| {
                chunk_tx_for_session.send_chunk(chunk);
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
pub(crate) async fn serial_debug_close(
    app: AppHandle,
    state: State<'_, DebugState>,
) -> Result<(), String> {
    let _lifecycle = SerialDebugLifecycleReservation::acquire(&state.lifecycle_busy)?;
    let session = {
        let mut guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        guard.take()
    };
    let bridge = state
        .chunk_bridge
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?
        .take();
    if let Some(session) = session {
        log::info!("[serial-debug] close");
        // h.join() blocks; run it off the async runtime thread.
        tauri::async_runtime::spawn_blocking(move || session.close())
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(bridge) = bridge {
        let archive = Arc::clone(&state.archive);
        let filters = Arc::clone(&state.filters);
        let sink = TauriSink { app: app.clone() };
        let closed_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            close_bridge_and_signal(bridge, &archive, &filters, &sink, || {
                let _ = closed_app.emit("serial-debug-closed", ());
            })
        })
        .await
        .map_err(|e| e.to_string())??;
    } else {
        finalize_and_signal(
            &state.archive,
            &state.filters,
            &TauriSink { app: app.clone() },
            || {
                let _ = app.emit("serial-debug-closed", ());
            },
        );
    }
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
    // Tx chunks bypass the bounded bridge queue and are archived right here, so
    // this is where their `archived_before` has to be read (same lock guard).
    let (completed, archived_before) = {
        let mut archive = state
            .archive
            .lock()
            .map_err(|_| "debug archive poisoned".to_string())?;
        let archived_before = archive.total_lines();
        (
            archive
                .append_chunk(&chunk)
                .map_err(|e| format!("append serial debug tx chunk failed: {e}"))?,
            archived_before,
        )
    };
    serial_debug_ingest_lines(&TauriSink { app: app.clone() }, &state.filters, &completed);
    let _ = app.emit(
        "serial-debug-chunk",
        &ArchivedChunk {
            chunk,
            archived_before,
        },
    );
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
    for (def, stats) in clear_session_state(&state)? {
        emit_filter_update(&app, &def, &stats);
    }
    Ok(())
}

/// Returns the archive `line_no` the line was written as, or `None` when nothing
/// was written (the archive is capped). The frontend needs the number for the
/// same reason chunks carry `archived_before` (see [`ArchivedChunk`]): a sys line
/// is pushed to the live view before it is archived, so its position in the
/// archive is only known once this command answers. `None` means "not in the
/// archive at all", which is what keeps such a line out of the discard pass.
#[tauri::command]
pub(crate) fn serial_debug_append_sys_line(
    app: AppHandle,
    state: State<'_, DebugState>,
    ts_ms: u64,
    text: String,
) -> Result<Option<u64>, String> {
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
    let line_no = line.as_ref().map(|line| line.line_no);
    if let Some(line) = line {
        serial_debug_ingest_lines(&TauriSink { app: app.clone() }, &state.filters, &[line]);
    }
    Ok(line_no)
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
    let historical_idx_path = historical_idx_path(&state.archive_dir, &filter_id);
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
    use std::thread;
    use std::time::{Duration, Instant};
    #[derive(Clone, Default)]
    struct CloseTestSink(Arc<StdMutex<Vec<&'static str>>>);

    impl SerialDebugSink for CloseTestSink {
        fn chunk_batch(&self, _chunks: Vec<ArchivedChunk>) {
            self.0.lock().unwrap().push("batch");
        }

        fn chunks_dropped(&self, _dropped_bytes: u64, _archived_before: u64) {
            self.0.lock().unwrap().push("dropped");
        }

        fn archive_capped(&self, _limit_mib: u64, _archived_before: u64) {
            self.0.lock().unwrap().push("capped");
        }

        fn filter_updated(
            &self,
            _def: SerialDebugFilterDefinition,
            _stats: SerialDebugFilterStats,
        ) {
            self.0.lock().unwrap().push("filter");
        }
    }

    #[test]
    fn lifecycle_reservation_rejects_overlapping_open_or_close_and_releases_on_drop() {
        let busy = Arc::new(AtomicBool::new(false));
        let first = SerialDebugLifecycleReservation::acquire(&busy).unwrap();
        assert_eq!(
            SerialDebugLifecycleReservation::acquire(&busy)
                .err()
                .as_deref(),
            Some("serial debug lifecycle operation in progress")
        );
        drop(first);
        assert!(SerialDebugLifecycleReservation::acquire(&busy).is_ok());
    }

    fn clear_test_state(dir: &Path) -> DebugState {
        DebugState {
            session: Arc::new(StdMutex::new(None)),
            archive: Arc::new(StdMutex::new(SerialDebugArchive::create(dir).unwrap())),
            filters: Arc::new(StdMutex::new(SerialDebugFilterIndex::create(dir).unwrap())),
            chunk_bridge: Arc::new(StdMutex::new(None)),
            generation: Arc::new(SerialDebugGeneration::default()),
            lifecycle_busy: Arc::new(AtomicBool::new(false)),
            archive_dir: dir.to_path_buf(),
        }
    }

    fn append_test_line(state: &DebugState, text: &[u8]) {
        state
            .archive
            .lock()
            .unwrap()
            .append_chunk(&DebugChunk {
                direction: tyutool_core::Direction::Rx,
                ts_ms: 1,
                bytes: text.to_vec(),
            })
            .unwrap();
    }

    #[test]
    fn desktop_clear_rejects_during_close_without_mutating_archive_then_clears_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let state = clear_test_state(dir.path());
        let events = Arc::new(StdMutex::new(Vec::new()));
        let bridge = serial_debug_spawn_chunk_bridge(
            CloseTestSink(Arc::clone(&events)),
            Arc::clone(&state.archive),
            Arc::clone(&state.filters),
            Arc::clone(&state.generation),
        );
        bridge.send_chunk(DebugChunk {
            direction: tyutool_core::Direction::Rx,
            ts_ms: 1,
            bytes: b"old-tail".to_vec(),
        });
        *state.chunk_bridge.lock().unwrap() = Some(bridge);

        // Follow the close command's order: reserve lifecycle, then take the
        // bridge out of state before awaiting its drain.
        let close_reservation =
            SerialDebugLifecycleReservation::acquire(&state.lifecycle_busy).unwrap();
        let bridge = state.chunk_bridge.lock().unwrap().take().unwrap();

        assert_eq!(
            clear_session_state(&state).unwrap_err(),
            "serial debug lifecycle operation in progress"
        );
        assert_eq!(state.archive.lock().unwrap().total_lines(), 0);
        assert_eq!(state.generation.current(), 0);

        close_bridge_and_signal(
            bridge,
            &state.archive,
            &state.filters,
            &CloseTestSink(Arc::clone(&events)),
            || events.lock().unwrap().push("closed"),
        )
        .unwrap();
        assert_eq!(state.archive.lock().unwrap().total_lines(), 1);
        assert_eq!(
            state
                .archive
                .lock()
                .unwrap()
                .read_page(0, 10)
                .unwrap()
                .items[0]
                .text,
            "old-tail"
        );

        drop(close_reservation);
        clear_session_state(&state).unwrap();
        assert_eq!(state.archive.lock().unwrap().total_lines(), 0);
    }

    #[test]
    fn desktop_clear_holds_lifecycle_until_archive_clear_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(clear_test_state(dir.path()));
        append_test_line(&state, b"old-tail\n");
        let archive_guard = state.archive.lock().unwrap();

        let clear_state = Arc::clone(&state);
        let clear = thread::spawn(move || clear_session_state(&clear_state));
        let deadline = Instant::now() + Duration::from_secs(1);
        while !state.lifecycle_busy.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            state.lifecycle_busy.load(Ordering::Acquire),
            "clear should reserve lifecycle before waiting for the archive lock"
        );

        // Open, close and clear all call this same reservation acquisition, so
        // neither lifecycle command can enter while clear is blocked here.
        assert!(SerialDebugLifecycleReservation::acquire(&state.lifecycle_busy).is_err());
        assert_eq!(archive_guard.total_lines(), 1);
        drop(archive_guard);

        clear.join().unwrap().unwrap();
        assert_eq!(state.archive.lock().unwrap().total_lines(), 0);
        assert_eq!(state.generation.current(), 1);
    }

    #[test]
    fn desktop_close_path_waits_for_bridge_drain_before_finalizing_and_signaling() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(StdMutex::new(
            SerialDebugArchive::create(dir.path()).unwrap(),
        ));
        let filters = Arc::new(StdMutex::new(
            SerialDebugFilterIndex::create(dir.path()).unwrap(),
        ));
        let events = Arc::new(StdMutex::new(Vec::new()));
        let bridge = serial_debug_spawn_chunk_bridge(
            CloseTestSink(Arc::clone(&events)),
            Arc::clone(&archive),
            Arc::clone(&filters),
            Arc::new(SerialDebugGeneration::default()),
        );
        bridge.send_chunk(DebugChunk {
            direction: tyutool_core::Direction::Rx,
            ts_ms: 1,
            bytes: b"final-tail".to_vec(),
        });

        close_bridge_and_signal(
            bridge,
            &archive,
            &filters,
            &CloseTestSink(Arc::clone(&events)),
            || {
                events.lock().unwrap().push("closed");
            },
        )
        .unwrap();

        assert_eq!(*events.lock().unwrap(), vec!["batch", "closed"]);
        let lines = archive.lock().unwrap().read_page(0, 10).unwrap().items;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "final-tail");
    }

    #[test]
    fn desktop_close_signals_completion_when_already_closed() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(StdMutex::new(
            SerialDebugArchive::create(dir.path()).unwrap(),
        ));
        let filters = Arc::new(StdMutex::new(
            SerialDebugFilterIndex::create(dir.path()).unwrap(),
        ));
        let events = Arc::new(StdMutex::new(Vec::new()));
        finalize_and_signal(
            &archive,
            &filters,
            &CloseTestSink(Arc::clone(&events)),
            || events.lock().unwrap().push("closed"),
        );
        assert_eq!(*events.lock().unwrap(), vec!["closed"]);
    }

    #[test]
    fn serial_debug_device_reset_session_requires_open_session() {
        let err = serial_debug_device_reset_session(None, "T5AI").unwrap_err();
        assert_eq!(err, "serial debug not open");
    }

    /// Regression test for the bug where the backfill `.historical.idx` path
    /// was derived from the global `serial_debug_archive_dir()` primary
    /// instead of the directory `create_serial_debug_state_resilient`
    /// actually used. When startup falls back to a pid-scoped subdirectory,
    /// the derived path must land inside that fallback, not the primary —
    /// otherwise the write targets a directory already known to be
    /// unwritable, and even when it isn't, the index ends up split from the
    /// archive it indexes.
    #[test]
    fn historical_idx_path_uses_the_actual_archive_dir_not_the_global_primary() {
        let primary = tyutool_core::serial_debug_archive_dir();
        let fallback = primary.join(format!("pid-{}", std::process::id()));

        let path = historical_idx_path(&fallback, "abc123");

        assert!(
            path.starts_with(&fallback),
            "historical idx path {path:?} must live under the actual (fallback) dir {fallback:?}"
        );
        assert_ne!(
            path.parent().unwrap(),
            primary,
            "historical idx path must not fall back to the global primary dir"
        );
    }
}
