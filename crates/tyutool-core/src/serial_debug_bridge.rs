//! The chunk bridge that carries serial-debug output from the reader thread to a
//! UI, shared by every host that has one.
//!
//! Device output arrives line-by-line but must reach the UI in coalesced batches
//! (`SERIAL_DEBUG_CHUNK_FLUSH_*`) so a chatty device cannot flood the transport.
//! That batching, the bounded queue in front of it, the archive bookkeeping and
//! the three notices the user has to see (batch, dropped, capped) are host
//! independent — only the *sink* differs: `src-tauri` emits Tauri events to the
//! webview, `tyutool-serve` sends `ServerMessage`s down a WebSocket.
//!
//! Both hosts previously carried their own copy of all of it. The copies had
//! already drifted (one grew a parameter it only passed to `let _ =`), and the
//! drop-report and archive-cap paths are exactly the kind of rare,
//! hard-to-reproduce behaviour where a fix landing in one copy and not the other
//! stays invisible until a user hits it. Implement [`SerialDebugSink`] and the
//! two hosts behave identically by construction.

use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::serial_debug::{
    serial_debug_archive_cap_limit_mib, serial_debug_now_ms, DebugChunk, Direction,
    SerialDebugArchive, SerialDebugChunkBatchBuffer, SerialDebugDropCounter, SerialDebugDropReport,
    SerialDebugFilterDefinition, SerialDebugFilterIndex, SerialDebugFilterStats,
    SerialDebugGeneration, SerialDebugLine,
};

/// How long a partially-filled batch may wait before it is flushed anyway. Also
/// the bridge thread's poll interval, so the drop check runs at least this often
/// even on a silent port.
pub const SERIAL_DEBUG_CHUNK_FLUSH_MS: u64 = 12;
/// Flush early once a batch reaches this many bytes, so a fast device gets
/// throughput instead of 12 ms of latency per 32 KiB.
pub const SERIAL_DEBUG_CHUNK_FLUSH_BYTES: usize = 32 * 1024;
/// Bound the bridge queue so sustained ingress can't grow process memory without
/// limit when archive/filter/UI consumption temporarily lags behind the serial
/// reader.
pub const SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY: usize = 256;

/// One chunk on its way to the UI, plus the number of lines the session archive
/// held *before* this chunk was appended to it.
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
///
/// Flattened into the `chunk` object, which already serialises camelCase
/// (`tsMs`), so the field reaches the frontend as `archivedBefore` with no
/// mapping at either transport boundary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedChunk {
    #[serde(flatten)]
    pub chunk: DebugChunk,
    pub archived_before: u64,
}

/// Where a host's serial-debug output goes. One implementation per transport.
///
/// Every method is fire-and-forget: a closed webview or a dropped WebSocket is
/// normal teardown, not something the bridge can act on, and the bridge thread
/// must not stall on it either way. Implementations must therefore neither block
/// nor panic. `Send + 'static` because the bridge owns one on its own thread.
pub trait SerialDebugSink: Send + 'static {
    /// One coalesced batch of device output, in arrival order.
    fn chunk_batch(&self, chunks: Vec<ArchivedChunk>);
    /// Device output the bounded queue could not accept. `archived_before` is the
    /// position of the gap lines this notice belongs to.
    fn chunks_dropped(&self, dropped_bytes: u64, archived_before: u64);
    /// The session archive hit its size cap and stopped recording.
    fn archive_capped(&self, limit_mib: u64, archived_before: u64);
    /// A filter's match count moved.
    fn filter_updated(&self, def: SerialDebugFilterDefinition, stats: SerialDebugFilterStats);
}

enum SerialDebugChunkBridgeMessage {
    Chunk {
        generation: u64,
        gap_before: Option<SerialDebugDropReport>,
        chunk: DebugChunk,
    },
    Reset {
        generation: u64,
        ack: SyncSender<()>,
    },
    /// Drain and stop, acknowledged. Hosts that tear the bridge down by dropping
    /// every handle get the same drain via `Disconnected` instead and never send
    /// this.
    Shutdown { ack: SyncSender<()> },
}

#[derive(Clone)]
pub struct SerialDebugChunkBridgeHandle {
    generation: Arc<SerialDebugGeneration>,
    send_lock: Arc<Mutex<()>>,
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
    pub fn send_chunk(&self, chunk: DebugChunk) {
        let _guard = self.send_lock.lock().unwrap();
        let bytes = chunk.bytes.len();
        let gap_before = self.drops.take_pending();
        match self.tx.try_send(SerialDebugChunkBridgeMessage::Chunk {
            generation: self.generation.current(),
            gap_before,
            chunk,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(message)) => {
                if let SerialDebugChunkBridgeMessage::Chunk {
                    gap_before: Some(report),
                    ..
                } = message
                {
                    self.drops.restore(report, serial_debug_now_ms());
                }
                self.drops.record(bytes, serial_debug_now_ms());
            }
            // The bridge thread is gone; the session is being torn down.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// Discard everything in flight and start a new generation, so output from
    /// the log the user just cleared cannot land in the new one.
    pub fn reset(&self) -> Result<u64, String> {
        let _guard = self.send_lock.lock().unwrap();
        let generation = self.generation.advance();
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        self.tx
            .send(SerialDebugChunkBridgeMessage::Reset {
                generation,
                ack: ack_tx,
            })
            .map_err(|e| e.to_string())?;
        ack_rx.recv().map_err(|e| e.to_string())?;
        Ok(generation)
    }

    /// Flush, report any last drops, and stop — waiting until the thread has done
    /// so.
    pub fn shutdown(self) -> Result<(), String> {
        let _guard = self.send_lock.lock().unwrap();
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        self.tx
            .send(SerialDebugChunkBridgeMessage::Shutdown { ack: ack_tx })
            .map_err(|e| e.to_string())?;
        ack_rx.recv().map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Every line the archive accepts passes through [`serial_debug_ingest_lines`],
/// which makes it the one place that can spot the archive-cap sentinel. The live
/// view never sees archived lines — it re-splits the raw chunk payloads itself —
/// so without this notice the cap would only ever exist in the archive file and
/// the user would watch the log keep scrolling with no hint that recording had
/// stopped.
fn emit_archive_cap_notice(sink: &impl SerialDebugSink, lines: &[SerialDebugLine]) {
    if let Some((limit_mib, line_no)) = lines
        .iter()
        .find_map(|line| serial_debug_archive_cap_limit_mib(line).map(|mib| (mib, line.line_no)))
    {
        // The sentinel is an archive line like any other, so its own position is
        // exactly `line_no - 1`.
        sink.archive_capped(limit_mib, line_no.saturating_sub(1));
    }
}

/// Feed newly-archived lines to the filter index and announce whatever moved.
pub fn serial_debug_ingest_lines(
    sink: &impl SerialDebugSink,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
    lines: &[SerialDebugLine],
) {
    if lines.is_empty() {
        return;
    }
    emit_archive_cap_notice(sink, lines);
    // One lock acquisition, not two: this runs per flushed batch (every 12 ms on
    // a busy port), and taking the lock again to look the definitions up would
    // also let a `filter_remove` land in between and silently swallow the update.
    let updates = {
        let mut guard = match filters.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let moved = guard.ingest_completed_lines(lines).unwrap_or_default();
        moved
            .into_iter()
            .filter_map(|stats| guard.definition(&stats.filter_id).map(|def| (def, stats)))
            .collect::<Vec<_>>()
    };
    // Emitted outside the guard: a sink must not be able to hold the filter index
    // for as long as its transport takes.
    for (def, stats) in updates {
        sink.filter_updated(def, stats);
    }
}

/// Archive one batch of chunks and hand it to the sink.
pub fn serial_debug_flush_chunks(
    sink: &impl SerialDebugSink,
    archive: &Arc<Mutex<SerialDebugArchive>>,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
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
    serial_debug_ingest_lines(sink, filters, &completed);
    sink.chunk_batch(archived);
}

/// Surface one coalesced burst of dropped chunks: a `log::warn!` for the
/// developer, a gap in the archive and a notice for the user.
///
/// Whatever is buffered is flushed first — those chunks arrived before the gap,
/// and emitting them afterwards would put the notice in the wrong place in the
/// live view.
pub fn serial_debug_report_drops(
    sink: &impl SerialDebugSink,
    archive: &Arc<Mutex<SerialDebugArchive>>,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
    pending: &mut SerialDebugChunkBatchBuffer,
    report: SerialDebugDropReport,
) {
    serial_debug_flush_chunks(sink, archive, filters, pending.take());
    log::warn!(
        "[serial-debug] chunk bridge queue full (capacity {}): dropped {} chunk(s) / {} byte(s) \
         of device output",
        SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY,
        report.chunks,
        report.bytes
    );
    // Only the reader thread's Rx chunks travel the bounded queue: the Tx path
    // writes straight to the archive.
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
    serial_debug_ingest_lines(sink, filters, &lines);
    sink.chunks_dropped(report.bytes, archived_before);
}

/// Cut the tail the device never terminated into the archive, at the end of a
/// session. Returns the lines written (empty when nothing was buffered) so the
/// caller can ingest them like any other archive line.
///
/// Nothing else closes that buffer: `append_chunk` only cuts on a newline and
/// `append_gap` only runs when a chunk is dropped, so a prompt or a progress bar
/// — output the device deliberately leaves unterminated — would go down with the
/// port and appear in neither the live view nor the archive.
pub fn serial_debug_finalize_pending(
    archive: &Arc<Mutex<SerialDebugArchive>>,
) -> Vec<SerialDebugLine> {
    let mut guard = match archive.lock() {
        Ok(guard) => guard,
        Err(_) => return Vec::new(),
    };
    guard
        .finalize_pending_lines(serial_debug_now_ms())
        .unwrap_or_default()
}

/// Stop the producer bridge, wait for its queue to drain, then archive and
/// ingest the final unterminated line. Hosts must not finalize before this
/// returns: chunks still held by the bridge belong before that tail.
pub fn serial_debug_shutdown_and_finalize<S: SerialDebugSink>(
    bridge: SerialDebugChunkBridgeHandle,
    archive: &Arc<Mutex<SerialDebugArchive>>,
    filters: &Arc<Mutex<SerialDebugFilterIndex>>,
    sink: &S,
) -> Result<(), String> {
    bridge.shutdown()?;
    let lines = serial_debug_finalize_pending(archive);
    serial_debug_ingest_lines(sink, filters, &lines);
    Ok(())
}

/// Start the bridge thread and return the handle the reader thread pushes into.
pub fn serial_debug_spawn_chunk_bridge<S: SerialDebugSink>(
    sink: S,
    archive: Arc<Mutex<SerialDebugArchive>>,
    filters: Arc<Mutex<SerialDebugFilterIndex>>,
    generation: Arc<SerialDebugGeneration>,
) -> SerialDebugChunkBridgeHandle {
    let (tx, rx) =
        mpsc::sync_channel::<SerialDebugChunkBridgeMessage>(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY);
    let drops = Arc::new(SerialDebugDropCounter::default());
    let handle = SerialDebugChunkBridgeHandle {
        generation: Arc::clone(&generation),
        send_lock: Arc::new(Mutex::new(())),
        tx,
        drops: Arc::clone(&drops),
    };
    let send_lock = Arc::clone(&handle.send_lock);
    std::thread::spawn(move || {
        let mut pending = SerialDebugChunkBatchBuffer::new();
        let mut active_generation = generation.current();
        let mut deferred = None;
        loop {
            let message = match deferred.take() {
                Some(message) => Ok(message),
                None => rx.recv_timeout(Duration::from_millis(SERIAL_DEBUG_CHUNK_FLUSH_MS)),
            };
            match message {
                Ok(SerialDebugChunkBridgeMessage::Chunk {
                    generation,
                    gap_before,
                    chunk,
                }) => {
                    if generation != active_generation {
                        if generation < active_generation {
                            continue;
                        }
                        let _ = pending.take();
                        active_generation = generation;
                    }
                    if let Some(report) = gap_before {
                        serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
                        serial_debug_report_drops(&sink, &archive, &filters, &mut pending, report);
                    }
                    pending.push(chunk);
                    // A busy port can keep recv_timeout from ever timing out.
                    // Check the batch age on arrivals too, or small continuous
                    // chunks wait until 32 KiB accumulate before reaching the UI.
                    if pending.should_flush_bytes(SERIAL_DEBUG_CHUNK_FLUSH_BYTES)
                        || pending.should_flush_elapsed(Duration::from_millis(
                            SERIAL_DEBUG_CHUNK_FLUSH_MS,
                        ))
                    {
                        serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
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
                Ok(SerialDebugChunkBridgeMessage::Shutdown { ack }) => {
                    serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
                    if let Some(report) = drops.take_pending() {
                        serial_debug_report_drops(&sink, &archive, &filters, &mut pending, report);
                    }
                    let _ = ack.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if pending
                        .should_flush_elapsed(Duration::from_millis(SERIAL_DEBUG_CHUNK_FLUSH_MS))
                    {
                        serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
                    }
                    // Drops without a following accepted chunk still need a
                    // timely report. Only place their gap after proving the
                    // queue is empty while the producer/reset path is idle.
                    let mut idle_report = None;
                    if let Ok(_producer_guard) = send_lock.try_lock() {
                        match rx.try_recv() {
                            Ok(message) => deferred = Some(message),
                            Err(mpsc::TryRecvError::Disconnected) => {
                                serial_debug_flush_chunks(
                                    &sink,
                                    &archive,
                                    &filters,
                                    pending.take(),
                                );
                                if let Some(report) = drops.take_pending() {
                                    serial_debug_report_drops(
                                        &sink,
                                        &archive,
                                        &filters,
                                        &mut pending,
                                        report,
                                    );
                                }
                                return;
                            }
                            Err(mpsc::TryRecvError::Empty) => {
                                idle_report = drops.take_report(serial_debug_now_ms());
                            }
                        }
                    }
                    if let Some(report) = idle_report {
                        serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
                        serial_debug_report_drops(&sink, &archive, &filters, &mut pending, report);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    serial_debug_flush_chunks(&sink, &archive, &filters, pending.take());
                    // A burst still aggregating at teardown is reported anyway —
                    // it is the last thing the user needs to know about the log
                    // they are about to read.
                    if let Some(report) = drops.take_pending() {
                        serial_debug_report_drops(&sink, &archive, &filters, &mut pending, report);
                    }
                    return;
                }
            }
        }
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial_debug::{
        serial_debug_archive_cap_sentinel, serial_debug_chunk_drop_sentinel, LogDirection,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    /// One temp dir per test, unique even when two land in the same millisecond.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tyutool-bridge-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[derive(Default)]
    struct Recorded {
        batches: Vec<Vec<ArchivedChunk>>,
        dropped: Vec<(u64, u64)>,
        capped: Vec<(u64, u64)>,
        filters: Vec<(String, u64)>,
    }

    #[derive(Clone, Default)]
    struct TestSink(Arc<Mutex<Recorded>>);

    impl TestSink {
        fn recorded(&self) -> std::sync::MutexGuard<'_, Recorded> {
            self.0.lock().unwrap()
        }
    }

    impl SerialDebugSink for TestSink {
        fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
            self.recorded().batches.push(chunks);
        }
        fn chunks_dropped(&self, dropped_bytes: u64, archived_before: u64) {
            self.recorded()
                .dropped
                .push((dropped_bytes, archived_before));
        }
        fn archive_capped(&self, limit_mib: u64, archived_before: u64) {
            self.recorded().capped.push((limit_mib, archived_before));
        }
        fn filter_updated(&self, def: SerialDebugFilterDefinition, stats: SerialDebugFilterStats) {
            self.recorded()
                .filters
                .push((def.keyword, stats.total_matches));
        }
    }

    fn rx(bytes: &[u8]) -> DebugChunk {
        DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1,
            bytes: bytes.to_vec(),
        }
    }

    /// Closing the port is the last moment the bytes the device printed without a
    /// trailing newline can be saved — a `login: ` prompt, a progress bar.
    /// Nothing else in the archive ever cuts them.
    ///
    /// Both hosts used to carry their own copy of this test against their own copy
    /// of the function; there is one of each now.
    #[test]
    fn closing_a_session_archives_the_unterminated_tail() {
        let dir = scratch_dir("close-tail");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        archive
            .lock()
            .unwrap()
            .append_chunk(&rx(b"login: "))
            .unwrap();
        assert_eq!(archive.lock().unwrap().total_lines(), 0, "no newline yet");

        let lines = serial_debug_finalize_pending(&archive);

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
        assert!(serial_debug_finalize_pending(&archive).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A full queue must cost us the chunk, not the reader thread.
    ///
    /// This test would hang rather than fail if `send_chunk` went back to a
    /// blocking `send`: nothing drains the receiver, so the third call would park
    /// forever — which is exactly what stalls the serial reader in production and
    /// lets the OS receive buffer overflow behind our back.
    #[test]
    fn full_bridge_queue_drops_chunks_instead_of_blocking_the_reader() {
        let (tx, receiver) = mpsc::sync_channel::<SerialDebugChunkBridgeMessage>(1);
        let handle = SerialDebugChunkBridgeHandle {
            generation: Arc::new(SerialDebugGeneration::default()),
            send_lock: Arc::new(Mutex::new(())),
            tx,
            drops: Arc::new(SerialDebugDropCounter::default()),
        };
        let chunk = |bytes: usize| rx(&vec![b'x'; bytes]);

        handle.send_chunk(chunk(4)); // fits the capacity-1 queue
        handle.send_chunk(chunk(8)); // dropped
        handle.send_chunk(chunk(16)); // dropped

        // One report for the whole burst, carrying the total loss.
        let report = handle.drops.take_pending().unwrap();
        assert_eq!(report.chunks, 2);
        assert_eq!(report.bytes, 24);
        assert!(handle.drops.take_pending().is_none());

        drop(receiver);
        // A disconnected queue is a closing session, not a data loss.
        handle.send_chunk(chunk(32));
        assert!(handle.drops.take_pending().is_none());
    }

    /// `archived_before` is per chunk, not per batch — it is what lets the
    /// frontend enable auto-save mid-session without duplicating or losing a line,
    /// so a batch-wide number would be wrong for every chunk but the first.
    #[test]
    fn flush_reports_the_archive_position_of_each_chunk() {
        let dir = scratch_dir("flush-positions");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();

        serial_debug_flush_chunks(
            &sink,
            &archive,
            &filters,
            vec![rx(b"first\n"), rx(b"second\n")],
        );

        let recorded = sink.recorded();
        assert_eq!(recorded.batches.len(), 1, "one batch, not one per chunk");
        let positions: Vec<u64> = recorded.batches[0]
            .iter()
            .map(|c| c.archived_before)
            .collect();
        assert_eq!(positions, vec![0, 1]);
        drop(recorded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An empty batch is not a notice: the bridge flushes on a timer, so most
    /// ticks on a quiet port have nothing to say.
    #[test]
    fn flushing_nothing_tells_the_sink_nothing() {
        let dir = scratch_dir("flush-empty");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();

        serial_debug_flush_chunks(&sink, &archive, &filters, Vec::new());

        assert!(sink.recorded().batches.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn continuous_small_chunks_reach_the_sink_before_the_byte_limit() {
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        let dir = scratch_dir("continuous-flush");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();
        let bridge = serial_debug_spawn_chunk_bridge(
            sink.clone(),
            archive,
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let producer_stop = Arc::clone(&stop);
        // Keep renewing recv_timeout without approaching the 32 KiB limit.
        // The sink must see output while the producer is still running.
        let producer = std::thread::spawn(move || {
            let mut sent_bytes = 0;
            while !producer_stop.load(Ordering::Relaxed)
                && sent_bytes + 2 < SERIAL_DEBUG_CHUNK_FLUSH_BYTES
            {
                bridge.send_chunk(rx(b"x\n"));
                sent_bytes += 2;
                std::thread::sleep(Duration::from_millis(1));
            }
            bridge
        });
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut delivered = false;
        while Instant::now() < deadline {
            if !sink.recorded().batches.is_empty() {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        stop.store(true, Ordering::Relaxed);
        producer.join().unwrap().shutdown().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            delivered,
            "continuous output waited for idle or the byte limit"
        );
    }

    #[test]
    fn shutdown_ack_allows_the_final_unterminated_chunk_to_be_archived() {
        let dir = scratch_dir("shutdown-tail");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let bridge = serial_debug_spawn_chunk_bridge(
            TestSink::default(),
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        bridge.send_chunk(rx(b"complete\nlast-tail"));

        // Hosts must wait for the drain acknowledgement before finalizing the
        // archive. Dropping a handle alone does not establish this ordering.
        bridge.shutdown().unwrap();
        serial_debug_finalize_pending(&archive);
        let texts: Vec<_> = archive
            .lock()
            .unwrap()
            .read_page(0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|line| line.text)
            .collect();
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(texts, vec!["complete", "last-tail"]);
    }

    #[test]
    fn a_full_queue_marks_the_gap_before_accepting_post_drop_bytes() {
        // Pause the consumer after its first batch so filling the bounded
        // queue and losing exactly one chunk require no timing assumptions.
        struct PausedSink {
            inner: TestSink,
            first: std::sync::Once,
            entered: SyncSender<()>,
            resume: mpsc::Receiver<()>,
        }
        impl SerialDebugSink for PausedSink {
            fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
                self.inner.chunk_batch(chunks);
                self.first.call_once(|| {
                    self.entered.send(()).unwrap();
                    self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
                });
            }
            fn chunks_dropped(&self, bytes: u64, before: u64) {
                self.inner.chunks_dropped(bytes, before);
            }
            fn archive_capped(&self, limit: u64, before: u64) {
                self.inner.archive_capped(limit, before);
            }
            fn filter_updated(
                &self,
                def: SerialDebugFilterDefinition,
                stats: SerialDebugFilterStats,
            ) {
                self.inner.filter_updated(def, stats);
            }
        }

        let dir = scratch_dir("ordered-gap");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let generation = Arc::new(SerialDebugGeneration::default());
        let bridge = serial_debug_spawn_chunk_bridge(
            PausedSink {
                inner: TestSink::default(),
                first: std::sync::Once::new(),
                entered: entered_tx,
                resume: resume_rx,
            },
            Arc::clone(&archive),
            filters,
            Arc::clone(&generation),
        );
        bridge.send_chunk(rx(b"before-gap"));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY {
            bridge.send_chunk(rx(b"x"));
        }
        bridge.send_chunk(rx(b"MISSING"));
        resume_tx.send(()).unwrap();
        // Wait for a slot for the first accepted post-gap chunk. A try_send
        // here could accidentally drop that chunk as well, obscuring the bug.
        bridge
            .tx
            .send(SerialDebugChunkBridgeMessage::Chunk {
                generation: generation.current(),
                gap_before: bridge.drops.take_pending(),
                chunk: rx(b"after-gap\n"),
            })
            .unwrap();
        bridge.shutdown().unwrap();

        let texts: Vec<_> = archive
            .lock()
            .unwrap()
            .read_page(0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|line| line.text)
            .collect();
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            texts,
            vec![
                format!(
                    "before-gap{}",
                    "x".repeat(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY)
                ),
                serial_debug_chunk_drop_sentinel(7),
                "after-gap".to_string(),
            ],
            "bytes either side of a dropped chunk must never become one line"
        );
    }

    #[test]
    fn two_full_queue_gaps_stay_between_their_adjacent_chunks() {
        struct PausedSink {
            inner: TestSink,
            calls: std::sync::atomic::AtomicUsize,
            entered: SyncSender<usize>,
            resume: mpsc::Receiver<()>,
        }
        impl SerialDebugSink for PausedSink {
            fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
                self.inner.chunk_batch(chunks);
                let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call <= 3 {
                    self.entered.send(call).unwrap();
                    self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }
            fn chunks_dropped(&self, bytes: u64, before: u64) {
                self.inner.chunks_dropped(bytes, before);
            }
            fn archive_capped(&self, limit: u64, before: u64) {
                self.inner.archive_capped(limit, before);
            }
            fn filter_updated(
                &self,
                def: SerialDebugFilterDefinition,
                stats: SerialDebugFilterStats,
            ) {
                self.inner.filter_updated(def, stats);
            }
        }

        let dir = scratch_dir("ordered-multiple-gaps");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (entered_tx, entered_rx) = mpsc::sync_channel(3);
        let (resume_tx, resume_rx) = mpsc::sync_channel(3);
        let generation = Arc::new(SerialDebugGeneration::default());
        let bridge = serial_debug_spawn_chunk_bridge(
            PausedSink {
                inner: TestSink::default(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                entered: entered_tx,
                resume: resume_rx,
            },
            Arc::clone(&archive),
            filters,
            Arc::clone(&generation),
        );

        bridge.send_chunk(rx(b"before-gap"));
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        for _ in 0..SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY {
            bridge.send_chunk(rx(b"x"));
        }
        bridge.send_chunk(rx(b"MISSING1"));
        resume_tx.send(()).unwrap();
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
        bridge
            .tx
            .send(SerialDebugChunkBridgeMessage::Chunk {
                generation: generation.current(),
                gap_before: bridge.drops.take_pending(),
                chunk: rx(b"after-gap1\n"),
            })
            .unwrap();

        for _ in 0..SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY {
            bridge.send_chunk(rx(b"y"));
        }
        bridge.send_chunk(rx(b"MISSING2"));
        resume_tx.send(()).unwrap();
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 3);
        bridge
            .tx
            .send(SerialDebugChunkBridgeMessage::Chunk {
                generation: generation.current(),
                gap_before: bridge.drops.take_pending(),
                chunk: rx(b"after-gap2\n"),
            })
            .unwrap();
        resume_tx.send(()).unwrap();
        bridge.shutdown().unwrap();

        let texts: Vec<_> = archive
            .lock()
            .unwrap()
            .read_page(0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|line| line.text)
            .collect();
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            texts,
            vec![
                format!(
                    "before-gap{}",
                    "x".repeat(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY)
                ),
                serial_debug_chunk_drop_sentinel(8),
                "after-gap1".to_string(),
                "y".repeat(SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY - 1),
                serial_debug_chunk_drop_sentinel(9),
                "after-gap2".to_string(),
            ]
        );
    }

    #[test]
    fn reader_does_not_wait_for_idle_drop_reporting() {
        struct BlockingSink {
            inner: TestSink,
            batch_entered: SyncSender<()>,
            batch_resume: mpsc::Receiver<()>,
            drop_entered: SyncSender<()>,
            drop_resume: mpsc::Receiver<()>,
            first_batch: std::sync::Once,
        }
        impl SerialDebugSink for BlockingSink {
            fn chunk_batch(&self, chunks: Vec<ArchivedChunk>) {
                self.inner.chunk_batch(chunks);
                self.first_batch.call_once(|| {
                    self.batch_entered.send(()).unwrap();
                    self.batch_resume
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                });
            }
            fn chunks_dropped(&self, bytes: u64, before: u64) {
                self.inner.chunks_dropped(bytes, before);
                self.drop_entered.send(()).unwrap();
                self.drop_resume
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            fn archive_capped(&self, limit: u64, before: u64) {
                self.inner.archive_capped(limit, before);
            }
            fn filter_updated(
                &self,
                def: SerialDebugFilterDefinition,
                stats: SerialDebugFilterStats,
            ) {
                self.inner.filter_updated(def, stats);
            }
        }

        let dir = scratch_dir("idle-drop-does-not-block-reader");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let (batch_entered_tx, batch_entered_rx) = mpsc::sync_channel(1);
        let (batch_resume_tx, batch_resume_rx) = mpsc::sync_channel(1);
        let (drop_entered_tx, drop_entered_rx) = mpsc::sync_channel(1);
        let (drop_resume_tx, drop_resume_rx) = mpsc::sync_channel(1);
        let bridge = serial_debug_spawn_chunk_bridge(
            BlockingSink {
                inner: TestSink::default(),
                batch_entered: batch_entered_tx,
                batch_resume: batch_resume_rx,
                drop_entered: drop_entered_tx,
                drop_resume: drop_resume_rx,
                first_batch: std::sync::Once::new(),
            },
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        bridge.send_chunk(rx(b"before-gap"));
        batch_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        for _ in 0..SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY {
            bridge.send_chunk(rx(b"x"));
        }
        bridge.send_chunk(rx(b"lost"));
        batch_resume_tx.send(()).unwrap();
        drop_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();

        let (returned_tx, returned_rx) = mpsc::sync_channel(1);
        let reader_bridge = bridge.clone();
        let reader = std::thread::spawn(move || {
            reader_bridge.send_chunk(rx(b"after-gap\n"));
            returned_tx.send(()).unwrap();
        });
        returned_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("reader send waited for drop-report sink I/O");
        reader.join().unwrap();
        drop_resume_tx.send(()).unwrap();
        bridge.shutdown().unwrap();

        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Dropping device output must cut the open archive line before the sentinel,
    /// or the bytes either side of the gap get concatenated into a line the device
    /// never printed. The notice then has to point at the gap it belongs to.
    #[test]
    fn reporting_drops_cuts_the_open_line_and_announces_the_gap() {
        let dir = scratch_dir("drop-report");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();
        let mut pending = SerialDebugChunkBatchBuffer::new();

        // A line still being received when the queue overflowed.
        archive
            .lock()
            .unwrap()
            .append_chunk(&rx(b"before-gap"))
            .unwrap();
        assert_eq!(archive.lock().unwrap().total_lines(), 0, "still open");

        serial_debug_report_drops(
            &sink,
            &archive,
            &filters,
            &mut pending,
            SerialDebugDropReport {
                chunks: 2,
                bytes: 4096,
            },
        );

        let texts: Vec<String> = archive
            .lock()
            .unwrap()
            .read_line_range(1, 10)
            .unwrap()
            .iter()
            .map(|l| l.text.clone())
            .collect();
        assert_eq!(texts.len(), 2, "the cut line, then the sentinel: {texts:?}");
        assert_eq!(texts[0], "before-gap");
        assert_eq!(texts[1], serial_debug_chunk_drop_sentinel(4096));

        // Both lines were written under one lock, so one position covers both.
        assert_eq!(sink.recorded().dropped, vec![(4096, 0)]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The live view is fed raw chunks and never reads the archive, so this notice
    /// is the only way the user learns recording stopped. It keys off the
    /// sentinel, which only the archive itself emits — identical text arriving
    /// from the device must not trigger it.
    #[test]
    fn the_archive_cap_sentinel_reaches_the_sink_but_device_output_does_not() {
        let dir = scratch_dir("cap-notice");
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();
        let line = |direction, text: String| SerialDebugLine {
            line_no: 7,
            ts_ms: 1,
            direction,
            text,
            raw_bytes: None,
        };

        serial_debug_ingest_lines(
            &sink,
            &filters,
            &[line(
                LogDirection::Sys,
                serial_debug_archive_cap_sentinel(64),
            )],
        );
        // The sentinel sits at line 7, so it is inside a backfill snapshot iff
        // that snapshot is >= 7, i.e. iff `archived_before` (6) < snapshot.
        assert_eq!(sink.recorded().capped, vec![(64, 6)]);

        serial_debug_ingest_lines(
            &sink,
            &filters,
            &[line(
                LogDirection::Rx,
                serial_debug_archive_cap_sentinel(64),
            )],
        );
        assert_eq!(
            sink.recorded().capped.len(),
            1,
            "device output must not count"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Filter counts move as lines are archived, and the sink hears about it —
    /// under a single lock acquisition, so a concurrent `filter_remove` cannot
    /// land between the ingest and the definition lookup and swallow the update.
    #[test]
    fn ingesting_lines_announces_the_filters_that_moved() {
        let dir = scratch_dir("filter-updates");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();
        filters
            .lock()
            .unwrap()
            .add_filter("boot".to_string(), false, "#f00".to_string(), 0)
            .unwrap();

        serial_debug_flush_chunks(
            &sink,
            &archive,
            &filters,
            vec![rx(b"boot ok\n"), rx(b"idle\n"), rx(b"boot again\n")],
        );

        assert_eq!(
            sink.recorded().filters,
            vec![("boot".to_string(), 2)],
            "one update carrying the final count, not one per matching line"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bridge_preserves_chunk_order_and_direction_within_a_batch() {
        let dir = scratch_dir("order-directions");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let sink = TestSink::default();
        serial_debug_flush_chunks(
            &sink,
            &archive,
            &filters,
            vec![
                rx(b"first\n"),
                DebugChunk {
                    direction: Direction::Tx,
                    ts_ms: 2,
                    bytes: b"sent\n".to_vec(),
                },
                rx(b"third\n"),
            ],
        );
        let lines = archive.lock().unwrap().read_page(0, 8).unwrap().items;
        assert_eq!(
            lines
                .iter()
                .map(|l| (l.direction, l.text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (LogDirection::Rx, "first"),
                (LogDirection::Tx, "sent"),
                (LogDirection::Rx, "third")
            ]
        );
        let recorded = sink.recorded();
        let batch = &recorded.batches[0];
        assert_eq!(
            batch
                .iter()
                .map(|c| (
                    c.chunk.direction,
                    c.chunk.bytes.as_slice(),
                    c.archived_before
                ))
                .collect::<Vec<_>>(),
            vec![
                (Direction::Rx, b"first\n".as_slice(), 0),
                (Direction::Tx, b"sent\n".as_slice(), 1),
                (Direction::Rx, b"third\n".as_slice(), 2)
            ]
        );
        drop(recorded);
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reset_discards_pre_reset_chunks_before_accepting_new_generation() {
        let dir = scratch_dir("reset-generation");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let bridge = serial_debug_spawn_chunk_bridge(
            TestSink::default(),
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        bridge.send_chunk(rx(b"old\n"));
        bridge.reset().unwrap();
        // Reset is a generation barrier; clearing persisted history is a
        // separate host action performed after the barrier acknowledges.
        archive.lock().unwrap().clear().unwrap();
        bridge.send_chunk(rx(b"new\n"));
        bridge.shutdown().unwrap();
        let texts: Vec<_> = archive
            .lock()
            .unwrap()
            .read_page(0, 8)
            .unwrap()
            .items
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, vec!["new"]);
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn send_after_consumer_shutdown_is_safely_ignored() {
        let dir = scratch_dir("send-disconnected");
        let archive = Arc::new(Mutex::new(SerialDebugArchive::create(&dir).unwrap()));
        let filters = Arc::new(Mutex::new(SerialDebugFilterIndex::create(&dir).unwrap()));
        let bridge = serial_debug_spawn_chunk_bridge(
            TestSink::default(),
            Arc::clone(&archive),
            filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        let late_sender = bridge.clone();
        bridge.shutdown().unwrap();
        late_sender.send_chunk(rx(b"after shutdown"));
        assert_eq!(archive.lock().unwrap().total_lines(), 0);
        drop(archive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn separate_bridge_archives_do_not_share_lines_or_positions() {
        let first_dir = scratch_dir("session-isolation-a");
        let second_dir = scratch_dir("session-isolation-b");
        let first_archive = Arc::new(Mutex::new(SerialDebugArchive::create(&first_dir).unwrap()));
        let second_archive = Arc::new(Mutex::new(SerialDebugArchive::create(&second_dir).unwrap()));
        let first_filters = Arc::new(Mutex::new(
            SerialDebugFilterIndex::create(&first_dir).unwrap(),
        ));
        let second_filters = Arc::new(Mutex::new(
            SerialDebugFilterIndex::create(&second_dir).unwrap(),
        ));
        let first = serial_debug_spawn_chunk_bridge(
            TestSink::default(),
            Arc::clone(&first_archive),
            first_filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        let second = serial_debug_spawn_chunk_bridge(
            TestSink::default(),
            Arc::clone(&second_archive),
            second_filters,
            Arc::new(SerialDebugGeneration::default()),
        );
        first.send_chunk(rx(b"alpha\n"));
        second.send_chunk(rx(b"beta\n"));
        first.shutdown().unwrap();
        second.shutdown().unwrap();
        assert_eq!(
            first_archive.lock().unwrap().read_page(0, 2).unwrap().items[0].text,
            "alpha"
        );
        assert_eq!(
            second_archive
                .lock()
                .unwrap()
                .read_page(0, 2)
                .unwrap()
                .items[0]
                .text,
            "beta"
        );
        drop((first_archive, second_archive));
        let _ = std::fs::remove_dir_all(first_dir);
        let _ = std::fs::remove_dir_all(second_dir);
    }
}
