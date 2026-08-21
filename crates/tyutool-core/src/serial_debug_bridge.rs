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
        chunk: DebugChunk,
    },
    Reset {
        generation: u64,
        ack: SyncSender<()>,
    },
    /// Drain and stop, acknowledged. Hosts that tear the bridge down by dropping
    /// every handle get the same drain via `Disconnected` instead and never send
    /// this.
    Shutdown {
        ack: SyncSender<()>,
    },
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
    std::thread::spawn(move || {
        let mut pending = SerialDebugChunkBatchBuffer::new();
        let mut active_generation = generation.current();
        loop {
            // Before every receive, so `recv_timeout`'s own tick is the poll
            // clock and no `continue` below can skip the check.
            if let Some(report) = drops.take_report(serial_debug_now_ms()) {
                serial_debug_report_drops(&sink, &archive, &filters, &mut pending, report);
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
}
