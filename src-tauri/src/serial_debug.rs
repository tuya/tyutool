//! Serial-debug session: open/close, send, the chunk bridge that batches device
//! output for the UI, filters, and device reset.
//!
//! The chunk bridge is why this is more than a thin command wrapper: device
//! output arrives line-by-line but reaches the webview in coalesced chunks
//! (`SERIAL_DEBUG_CHUNK_FLUSH_*`) so a chatty device cannot flood the IPC
//! channel.

use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use tyutool_core::{
    serial_debug_fail_backfill_if_current, serial_debug_finish_backfill_if_current,
    serial_debug_now_ms, serial_debug_scan_filter_matches, DebugChunk, DebugConfig, Direction,
    SerialDebugArchive, SerialDebugArchiveReader, SerialDebugChunkBatchBuffer,
    SerialDebugDropCounter, SerialDebugDropReport, SerialDebugFilterBackfillSnapshot,
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
    /// `archived_before` of the cap sentinel itself — see [`ArchivedChunk`].
    archived_before: u64,
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

/// One chunk on its way to the webview, plus the number of lines the session
/// archive held *before* this chunk was appended to it.
///
/// That number is what lets the frontend switch auto-save on mid-session without
/// either duplicating or losing a line. Auto-save enables in two halves — the
/// archive is paged into the file up to a snapshot `N`, the live queue continues
/// after it — and the frontend cannot otherwise tell which half a live line
/// belongs to: its own line counter and the archive's `line_no` diverge (the
/// archive freezes its numbering once capped, and never holds an unterminated
/// trailing line).
///
/// `archived_before` closes that gap exactly, because `append_chunk` archives a
/// whole chunk under one lock: no snapshot can land *inside* a chunk, so every
/// line the chunk produced is either wholly inside `N` or wholly after it.
/// A live line is therefore already in the backfilled half iff
/// `archived_before < N` — see `dropBackfilledAutoSaveLines` in
/// `src/stores/serial-debug.ts`.
///
/// Read under the same lock guard as the `append_chunk` it precedes; reading it
/// outside the guard would let another writer slip in between and make the
/// number a lie.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchivedChunk {
    #[serde(flatten)]
    chunk: DebugChunk,
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
    if let Some((limit_mib, line_no)) = lines.iter().find_map(|line| {
        tyutool_core::serial_debug_archive_cap_limit_mib(line).map(|mib| (mib, line.line_no))
    }) {
        let _ = app.emit(
            "serial-debug-archive-capped",
            &ArchiveCappedPayload {
                limit_mib,
                // The sentinel is an archive line like any other, so its own
                // position is exactly `line_no - 1`.
                archived_before: line_no.saturating_sub(1),
            },
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
    let (completed, archived) = {
        let mut guard = match archive.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let mut completed = Vec::new();
        let mut archived = Vec::with_capacity(chunks.len());
        // One `archived_before` per chunk, not one per batch: per-chunk only
        // needs `append_chunk` to be atomic, which the archive guarantees on its
        // own, whereas a per-batch number would additionally rely on this loop
        // holding the lock for the whole batch.
        for chunk in chunks {
            let archived_before = guard.total_lines();
            completed.extend(guard.append_chunk(&chunk).unwrap_or_default());
            archived.push(ArchivedChunk {
                chunk,
                archived_before,
            });
        }
        (completed, archived)
    };
    ingest_serial_debug_lines(app, archive, filters, &completed);
    let _ = app.emit("serial-debug-chunk-batch", &archived);
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
    drops: Arc<SerialDebugDropCounter>,
}

impl SerialDebugChunkBridgeHandle {
    /// Hand one chunk to the bridge, or account for it as lost.
    ///
    /// `try_send`, never `send`: this runs on the serial reader thread, which is
    /// the only thread draining the OS/driver receive buffer, and the port runs
    /// without flow control. Blocking here therefore applies no backpressure to
    /// the *device* — it just stops the buffer being drained until the driver
    /// overflows and discards bytes we never saw, with no count, no error and
    /// nothing to show the user. Dropping the chunk here instead keeps the reader
    /// draining and moves the loss to a boundary we own: we know how many bytes
    /// went, we can close the archive line so the halves cannot be spliced, and
    /// we can tell the user (who can lower the baud rate or quieten the device).
    fn send_chunk(&self, chunk: DebugChunk) {
        let _guard = self.send_lock.lock().unwrap();
        let bytes = chunk.bytes.len();
        match self.tx.try_send(SerialDebugChunkBridgeMessage::Chunk {
            generation: self.generation.current(),
            chunk,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.drops.record(bytes, serial_debug_now_ms()),
            // The bridge thread is gone; the session is being torn down.
            Err(TrySendError::Disconnected(_)) => {}
        }
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

/// Surface one coalesced burst of dropped chunks: a `log::warn!` for the
/// developer, a gap in the archive and a `Sys` notice for the user.
///
/// Whatever is buffered is flushed first — those chunks arrived before the gap,
/// and emitting them afterwards would put the notice in the wrong place in the
/// live view.
fn report_serial_debug_drops(
    app: &AppHandle,
    archive: &Arc<StdMutex<SerialDebugArchive>>,
    filters: &Arc<StdMutex<SerialDebugFilterIndex>>,
    pending: &mut SerialDebugChunkBatchBuffer,
    report: SerialDebugDropReport,
) {
    flush_serial_debug_chunk(app, archive, filters, pending.take());
    log::warn!(
        "[serial-debug] chunk bridge queue full (capacity {}): dropped {} chunk(s) / {} byte(s) \
         of device output",
        SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY,
        report.chunks,
        report.bytes
    );
    // Only the reader thread's Rx chunks travel the bounded queue: the Tx path
    // (`serial_debug_send`) writes straight to the archive.
    let (lines, archived_before) = {
        let mut guard = match archive.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        // `append_gap` writes the cut-off partial line and the sentinel under one
        // lock, so one number covers both frontend lines — see [`ArchivedChunk`].
        let archived_before = guard.total_lines();
        (
            guard
                .append_gap(Direction::Rx, serial_debug_now_ms(), report.bytes)
                .unwrap_or_default(),
            archived_before,
        )
    };
    ingest_serial_debug_lines(app, archive, filters, &lines);
    let _ = app.emit(
        "serial-debug-chunks-dropped",
        &ChunksDroppedPayload {
            dropped_bytes: report.bytes,
            archived_before,
        },
    );
}

/// Cut the tail the device never terminated into the archive, at the end of a
/// session. Returns the lines written (empty when nothing was buffered) so the
/// caller can ingest them like any other archive line.
///
/// Nothing else closes that buffer: `append_chunk` only cuts on a newline and
/// `append_gap` only runs when a chunk is dropped, so a prompt or a progress bar
/// — output the device deliberately leaves unterminated — would go down with the
/// port and appear in neither the live view nor the archive.
fn finalize_serial_debug_pending(
    archive: &Arc<StdMutex<SerialDebugArchive>>,
) -> Vec<SerialDebugLine> {
    let mut guard = match archive.lock() {
        Ok(guard) => guard,
        Err(_) => return Vec::new(),
    };
    guard
        .finalize_pending_lines(serial_debug_now_ms())
        .unwrap_or_default()
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
    let drops = Arc::new(SerialDebugDropCounter::default());
    let handle = SerialDebugChunkBridgeHandle {
        generation: Arc::clone(&generation),
        send_lock: Arc::new(StdMutex::new(())),
        tx: tx.clone(),
        drops: Arc::clone(&drops),
    };
    std::thread::spawn(move || {
        let mut pending = SerialDebugChunkBatchBuffer::new();
        let mut active_generation = generation.current();
        loop {
            // Before every receive, so `recv_timeout`'s own tick is the poll
            // clock and no `continue` below can skip the check.
            if let Some(report) = drops.take_report(serial_debug_now_ms()) {
                report_serial_debug_drops(&app, &archive, &filters, &mut pending, report);
            }
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
                    // Drops from the cleared session belong to the log the user
                    // just discarded; reporting them into the new one would be a
                    // notice about a gap that is no longer there.
                    let _ = drops.take_pending();
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
                    // A burst still aggregating at teardown is reported anyway —
                    // it is the last thing the user needs to know about the log
                    // they are about to read.
                    if let Some(report) = drops.take_pending() {
                        report_serial_debug_drops(&app, &archive, &filters, &mut pending, report);
                    }
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
    // Cut after the bridge handle is dropped, which is what releases the bridge
    // thread's final flush, so the chunks it was still holding land in the
    // archive ahead of the tail. (`tyutool-serve`'s bridge acks its shutdown and
    // is therefore exact about that ordering; this one has no ack.)
    let lines = finalize_serial_debug_pending(&state.archive);
    ingest_serial_debug_lines(&app, &state.archive, &state.filters, &lines);
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
    ingest_serial_debug_lines(&app, &state.archive, &state.filters, &completed);
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
        ingest_serial_debug_lines(&app, &state.archive, &state.filters, &[line]);
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

    /// Closing the port is the last moment the bytes the device printed without
    /// a trailing newline can be saved — a `login: ` prompt, a progress bar.
    /// Nothing else in the archive ever cuts them.
    #[test]
    fn closing_a_session_archives_the_unterminated_tail() {
        let dir = std::env::temp_dir().join(format!(
            "tyutool-gui-serial-debug-close-{}-{}",
            std::process::id(),
            serial_debug_now_ms()
        ));
        let archive = Arc::new(StdMutex::new(SerialDebugArchive::create(&dir).unwrap()));
        archive
            .lock()
            .unwrap()
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"login: ".to_vec(),
            })
            .unwrap();
        assert_eq!(archive.lock().unwrap().total_lines(), 0, "no newline yet");

        let lines = finalize_serial_debug_pending(&archive);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "login: ");
        assert_eq!(
            archive
                .lock()
                .unwrap()
                .read_line_range(1, 10)
                .unwrap()
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>(),
            vec!["login: ".to_string()]
        );
        // Closing again has nothing left to cut: no empty line.
        assert!(finalize_serial_debug_pending(&archive).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A full queue must cost us the chunk, not the reader thread.
    ///
    /// This test would hang rather than fail if `send_chunk` went back to a
    /// blocking `send`: nothing drains `rx`, so the third call would park
    /// forever — which is exactly what stalls the serial reader in production
    /// and lets the OS receive buffer overflow behind our back.
    #[test]
    fn full_bridge_queue_drops_chunks_instead_of_blocking_the_reader() {
        let (tx, rx) = mpsc::sync_channel::<SerialDebugChunkBridgeMessage>(1);
        let handle = SerialDebugChunkBridgeHandle {
            generation: Arc::new(SerialDebugGeneration::default()),
            send_lock: Arc::new(StdMutex::new(())),
            tx,
            drops: Arc::new(SerialDebugDropCounter::default()),
        };
        let chunk = |bytes: usize| DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1,
            bytes: vec![b'x'; bytes],
        };

        handle.send_chunk(chunk(4)); // fits the capacity-1 queue
        handle.send_chunk(chunk(8)); // dropped
        handle.send_chunk(chunk(16)); // dropped

        // One report for the whole burst, carrying the total loss.
        let report = handle.drops.take_pending().unwrap();
        assert_eq!(report.chunks, 2);
        assert_eq!(report.bytes, 24);
        assert!(handle.drops.take_pending().is_none());

        drop(rx);
        // A disconnected queue is a closing session, not a data loss.
        handle.send_chunk(chunk(32));
        assert!(handle.drops.take_pending().is_none());
    }
}
