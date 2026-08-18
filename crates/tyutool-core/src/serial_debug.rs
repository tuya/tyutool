//! Interactive serial debug session — open a port, stream Rx chunks
//! to a callback, send bytes, handle disconnects. Used by the Tauri
//! `serial_debug_*` commands and the CLI `serve` WS handler.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::FlashError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfig {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Tx,
    Rx,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugChunk {
    pub direction: Direction,
    pub ts_ms: u64,
    pub bytes: Vec<u8>,
}

pub type ChunkCallback = Box<dyn Fn(DebugChunk) + Send + Sync>;
pub type DisconnectCallback = Box<dyn Fn(String) + Send + Sync>;

const ARCHIVE_INDEX_ENTRY_BYTES: u64 = 16;
const FILTER_MATCH_INDEX_ENTRY_BYTES: u64 = 8;
const MAX_PENDING_SERIAL_DEBUG_LINE_BYTES: usize = 4096;
const FILTER_BACKFILL_READ_BATCH_LINES: u64 = 512;
/// How many `.idx` entries one archive read pulls in a single `read_exact`
/// (16 KiB). A page is 400 lines, so a normal page costs exactly one index
/// read; the cap only bounds the buffer when a caller asks for a huge `limit`.
const ARCHIVE_READ_INDEX_BATCH_LINES: usize = 1024;
/// Byte budget for one bulk read out of the `.ndjson`. Consecutive archived
/// lines occupy a contiguous byte span (see [`read_line_run`]), so a page is
/// normally fetched with one `read_exact`; this splits the span when a caller
/// asks for more lines than the budget covers, so peak memory stays bounded
/// regardless of `limit`. A line is at most
/// `MAX_PENDING_SERIAL_DEBUG_LINE_BYTES` of payload, so any single line always
/// fits and the split always makes progress.
const ARCHIVE_READ_SPAN_BUDGET_BYTES: u64 = 4 * 1024 * 1024;
static SERIAL_DEBUG_SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

/// Compile-time cap for one session archive. The GUI/web setting overrides this
/// at runtime, but a device can flood the port before the setting is pushed
/// down (~508 KiB/s at 921600 baud), so the default has to be bounded on its
/// own. Mirrors `DEFAULT_ARCHIVE_LIMIT_MIB` in
/// `src/features/serial-debug/constants.ts`.
const DEFAULT_SERIAL_DEBUG_ARCHIVE_MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Bytes reserved below `max_bytes` so the "archive full" `Sys` line always
/// fits. Its encoded form is well under 300 B.
const ARCHIVE_CAP_NOTICE_HEADROOM_BYTES: u64 = 512;
/// Stem prefix shared by both halves of a session archive pair
/// (`<prefix><session-id>.ndjson` + `<prefix><session-id>.idx`).
const SERIAL_DEBUG_SESSION_FILE_PREFIX: &str = "serial-debug-session-";
/// Cross-session bounds for the archive directory, in the shape of
/// `prune_log_files`: file *pairs* and total bytes, whichever binds first.
const MAX_ARCHIVE_SESSIONS: usize = 20;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogDirection {
    Tx,
    Rx,
    Sys,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugLine {
    pub line_no: u64,
    pub ts_ms: u64,
    pub direction: LogDirection,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_bytes: Option<Vec<u8>>,
}

/// Opening delimiter of the archive-cap sentinel. `\u{1}` (SOH) is a C0 control
/// character: it never occurs in a translated UI string, which is the only other
/// source of `Sys` lines.
const ARCHIVE_CAP_SENTINEL_PREFIX: &str = "\u{1}tyutool:archive-capped:";
/// Opening delimiter of the dropped-chunk sentinel — same family, same
/// collision-safety rules, different payload (bytes lost, not the MiB cap).
const CHUNK_DROP_SENTINEL_PREFIX: &str = "\u{1}tyutool:chunks-dropped:";
const SENTINEL_SUFFIX: char = '\u{1}';

/// Text written into the archive in place of the first line that would have
/// broken the size cap, carrying the limit in MiB.
///
/// Why a sentinel rather than a finished sentence:
/// * the notice is user-visible, so its wording must come from the frontend
///   i18n catalogue (`serialDebug.log.archiveCapped`) — a Chinese UI must not
///   grow an English line;
/// * translating at read time means a session archive re-read after the user
///   switches language shows the notice in the *new* language, which a string
///   baked in at write time could never do.
///
/// Collision safety — a device log line can never be mistaken for this:
/// 1. only `Sys` lines are ever tested ([`serial_debug_archive_cap_limit_mib`]),
///    and device bytes always arrive as `Tx`/`Rx`, so device output cannot reach
///    the check at all;
/// 2. the remaining `Sys` producer is the frontend's own `appendSysLine`, which
///    passes translated catalogue strings — none of which contain U+0001;
/// 3. the match is on the whole string (prefix + digits + suffix), not a
///    substring, so a longer line that merely embeds the marker is not a match.
pub fn serial_debug_archive_cap_sentinel(limit_mib: u64) -> String {
    format!("{ARCHIVE_CAP_SENTINEL_PREFIX}{limit_mib}{SENTINEL_SUFFIX}")
}

/// Text written into the archive at a gap in the chunk stream, carrying the
/// number of bytes lost. Same reasoning and the same three collision-safety
/// layers as [`serial_debug_archive_cap_sentinel`].
pub fn serial_debug_chunk_drop_sentinel(dropped_bytes: u64) -> String {
    format!("{CHUNK_DROP_SENTINEL_PREFIX}{dropped_bytes}{SENTINEL_SUFFIX}")
}

/// The numeric payload of a `Sys` sentinel line built from `prefix`, or `None`
/// for any other line. The `Sys`-only / whole-string / digits-only rules live
/// here so every sentinel family gets all three.
fn sentinel_payload(line: &SerialDebugLine, prefix: &str) -> Option<u64> {
    if line.direction != LogDirection::Sys {
        return None;
    }
    let digits = line
        .text
        .strip_prefix(prefix)?
        .strip_suffix(SENTINEL_SUFFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The MiB limit carried by an archive-cap sentinel line, or `None` for any
/// other line. Mirrored in `src/features/serial-debug/archive-line-text.ts`.
pub fn serial_debug_archive_cap_limit_mib(line: &SerialDebugLine) -> Option<u64> {
    sentinel_payload(line, ARCHIVE_CAP_SENTINEL_PREFIX)
}

/// The byte count carried by a dropped-chunk sentinel line, or `None` for any
/// other line. Mirrored in `src/features/serial-debug/archive-line-text.ts`.
pub fn serial_debug_chunk_drop_bytes(line: &SerialDebugLine) -> Option<u64> {
    sentinel_payload(line, CHUNK_DROP_SENTINEL_PREFIX)
}

fn log_direction(direction: Direction) -> LogDirection {
    match direction {
        Direction::Tx => LogDirection::Tx,
        Direction::Rx => LogDirection::Rx,
    }
}

/// One coalesced report of chunks the bridge had to drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialDebugDropReport {
    pub chunks: u64,
    pub bytes: u64,
}

/// Quiet period after the last drop before a burst is reported. Without it a
/// sustained overload would emit one notice per dropped chunk — thousands of
/// them, which is noise, not information.
pub const SERIAL_DEBUG_DROP_QUIET_MS: u64 = 250;
/// Ceiling on how long a burst may keep absorbing drops before it is reported
/// anyway. A device that never stops printing would otherwise never go quiet,
/// and the user — the one person who can act on this by lowering the baud rate
/// or muting device-side logging — would be told nothing at all.
pub const SERIAL_DEBUG_DROP_BURST_MS: u64 = 2000;

/// Accumulates dropped chunks so one burst of loss becomes one user-visible
/// notice.
///
/// Written by the serial reader thread (which must never block) and read by the
/// bridge thread. Both counters are swapped independently, so a drop recorded
/// between the two swaps can have its chunk count and its byte count land in
/// different reports; nothing is ever lost, and the user-visible figure (bytes)
/// stays exact per report.
#[derive(Debug, Default)]
pub struct SerialDebugDropCounter {
    chunks: AtomicU64,
    bytes: AtomicU64,
    first_drop_ms: AtomicU64,
    last_drop_ms: AtomicU64,
}

impl SerialDebugDropCounter {
    /// Record one dropped chunk. Cheap and non-blocking by construction: this
    /// runs on the reader thread, the only thread draining the OS buffer.
    pub fn record(&self, bytes: usize, now_ms: u64) {
        self.chunks.fetch_add(1, Ordering::SeqCst);
        self.bytes.fetch_add(bytes as u64, Ordering::SeqCst);
        let _ = self
            .first_drop_ms
            .compare_exchange(0, now_ms, Ordering::SeqCst, Ordering::SeqCst);
        self.last_drop_ms.store(now_ms, Ordering::SeqCst);
    }

    /// The pending burst, if it is ready to report: either the loss has stopped
    /// ([`SERIAL_DEBUG_DROP_QUIET_MS`]) or it has been going on long enough that
    /// waiting for it to stop would keep the user in the dark
    /// ([`SERIAL_DEBUG_DROP_BURST_MS`]).
    pub fn take_report(&self, now_ms: u64) -> Option<SerialDebugDropReport> {
        if self.chunks.load(Ordering::SeqCst) == 0 && self.bytes.load(Ordering::SeqCst) == 0 {
            return None;
        }
        let quiet_for = now_ms.saturating_sub(self.last_drop_ms.load(Ordering::SeqCst));
        let burst_age = now_ms.saturating_sub(self.first_drop_ms.load(Ordering::SeqCst));
        if quiet_for < SERIAL_DEBUG_DROP_QUIET_MS && burst_age < SERIAL_DEBUG_DROP_BURST_MS {
            return None;
        }
        self.take_pending()
    }

    /// Drain whatever is pending regardless of timing — for teardown, so a burst
    /// that was still aggregating when the session ended is still reported, and
    /// for session clear, where the caller discards it.
    pub fn take_pending(&self) -> Option<SerialDebugDropReport> {
        let chunks = self.chunks.swap(0, Ordering::SeqCst);
        let bytes = self.bytes.swap(0, Ordering::SeqCst);
        self.first_drop_ms.store(0, Ordering::SeqCst);
        if chunks == 0 && bytes == 0 {
            return None;
        }
        Some(SerialDebugDropReport { chunks, bytes })
    }
}

pub struct SerialDebugArchive {
    root_dir: std::path::PathBuf,
    session_id: String,
    log_path: std::path::PathBuf,
    idx_path: std::path::PathBuf,
    log_writer: std::io::BufWriter<File>,
    idx_writer: std::io::BufWriter<File>,
    next_offset: u64,
    next_line_no: u64,
    /// Byte budget for `log_path`; 0 disables the cap.
    max_bytes: u64,
    /// Set once the budget is exhausted. Existing content is kept and new lines
    /// are dropped (`stopWriting`) — never rewritten, so line numbers and the
    /// `(line_no - 1) * 16` index arithmetic stay valid.
    capped: bool,
    pending_tx: Vec<u8>,
    pending_rx: Vec<u8>,
}

pub struct SerialDebugArchiveReader {
    log_path: std::path::PathBuf,
    idx_path: std::path::PathBuf,
}

pub struct SerialDebugChunkBatchBuffer {
    chunks: Vec<DebugChunk>,
    pending_bytes: usize,
    first_chunk_at: Option<Instant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugFilterDefinition {
    pub id: String,
    pub keyword: String,
    pub use_regex: bool,
    pub color: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SerialDebugFilterStatus {
    Pending,
    Backfilling,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugFilterStats {
    pub filter_id: String,
    pub status: SerialDebugFilterStatus,
    pub scanned_until_line_no: u64,
    pub total_lines_snapshot: u64,
    pub total_matches: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugFilterPage {
    pub filter_id: String,
    pub total_matches: u64,
    pub start: u64,
    pub items: Vec<SerialDebugLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugSessionPage {
    pub total_lines: u64,
    pub start: u64,
    pub items: Vec<SerialDebugLine>,
}

struct SerialDebugFilterEntry {
    def: SerialDebugFilterDefinition,
    status: SerialDebugFilterStatus,
    snapshot_total_lines: u64,
    scanned_until_line_no: u64,
    total_matches: u64,
    error: Option<String>,
    match_idx_path: std::path::PathBuf,
    match_idx_writer: std::io::BufWriter<File>,
    pending_live_match_idx_path: Option<std::path::PathBuf>,
    pending_live_match_idx_writer: Option<std::io::BufWriter<File>>,
    pending_live_matches: u64,
    pending_live_until_line_no: u64,
    regex: Option<regex::Regex>,
}

pub struct SerialDebugFilterIndex {
    root_dir: std::path::PathBuf,
    next_filter_id: u64,
    filters: HashMap<String, SerialDebugFilterEntry>,
}

#[derive(Debug, Clone)]
pub struct SerialDebugFilterBackfillSnapshot {
    pub filter_id: String,
    pub keyword: String,
    pub use_regex: bool,
    pub snapshot_total_lines: u64,
}

#[derive(Debug, Default)]
pub struct SerialDebugGeneration {
    current: AtomicU64,
}

type SerialDebugSessionFiles = (
    String,
    std::path::PathBuf,
    std::path::PathBuf,
    std::io::BufWriter<File>,
    std::io::BufWriter<File>,
);

impl SerialDebugGeneration {
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    pub fn advance(&self) -> u64 {
        self.current.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.current() == generation
    }
}

impl Default for SerialDebugFilterIndex {
    fn default() -> Self {
        let root_dir = std::env::temp_dir().join(format!(
            "tyutool-serial-debug-filters-{}-{}",
            std::process::id(),
            now_ms()
        ));
        Self::create(&root_dir).expect("create serial-debug filter index")
    }
}

fn open_filter_match_index_writer_with_options(
    path: &std::path::Path,
    truncate: bool,
    append: bool,
) -> std::io::Result<std::io::BufWriter<File>> {
    Ok(std::io::BufWriter::new(
        OpenOptions::new()
            .create(true)
            .truncate(truncate)
            .append(append)
            .write(true)
            .open(path)?,
    ))
}

fn open_filter_match_index_writer(
    path: &std::path::Path,
) -> std::io::Result<std::io::BufWriter<File>> {
    open_filter_match_index_writer_with_options(path, true, false)
}

fn open_filter_match_index_append_writer(
    path: &std::path::Path,
) -> std::io::Result<std::io::BufWriter<File>> {
    open_filter_match_index_writer_with_options(path, false, true)
}

impl SerialDebugFilterIndex {
    pub fn create(root_dir: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(root_dir)?;
        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            next_filter_id: 1,
            filters: HashMap::new(),
        })
    }

    fn filter_match_index_path(&self, filter_id: &str) -> std::path::PathBuf {
        self.root_dir
            .join(format!("serial-debug-filter-{filter_id}.idx"))
    }

    fn pending_live_filter_match_index_path(&self, filter_id: &str) -> std::path::PathBuf {
        self.root_dir
            .join(format!("serial-debug-filter-{filter_id}.live.idx"))
    }
}

struct ArchiveSessionFiles {
    paths: Vec<std::path::PathBuf>,
    bytes: u64,
    modified: SystemTime,
}

/// Delete the oldest `serial-debug-session-*` archives until the directory is
/// within both the pair-count and total-byte limits. Always keeps at least one.
///
/// Selection is keyed on the **stem prefix**, never on the extension: the same
/// directory also holds `serial-debug-filter-<id>.idx` (and `.live.idx`) match
/// indexes belonging to *live* filters, and an `extension == "idx"` sweep would
/// delete those out from under a running session.
///
/// Both halves of a pair are removed together — an orphan `.idx` indexes
/// nothing and an orphan `.ndjson` cannot be paged.
///
/// Ordering is by mtime (oldest first), not by filename as in
/// `prune_log_files`: an archive that is still being written keeps its mtime
/// fresh, so mtime ordering protects the live archives of this process *and* of
/// the other `tyutool-serve` WebSocket connections that share this directory.
fn prune_serial_debug_archives(root_dir: &std::path::Path) {
    let mut sessions: HashMap<String, ArchiveSessionFiles> = HashMap::new();
    let entries = match std::fs::read_dir(root_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !stem.starts_with(SERIAL_DEBUG_SESSION_FILE_PREFIX) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let entry = sessions
            .entry(stem.to_string())
            .or_insert_with(|| ArchiveSessionFiles {
                paths: Vec::new(),
                bytes: 0,
                modified: UNIX_EPOCH,
            });
        entry.paths.push(path);
        entry.bytes = entry.bytes.saturating_add(meta.len());
        entry.modified = entry.modified.max(modified);
    }

    let mut sessions = sessions.into_values().collect::<Vec<_>>();
    sessions.sort_by_key(|session| session.modified);

    let mut count = sessions.len();
    let mut total = sessions
        .iter()
        .fold(0u64, |acc, session| acc.saturating_add(session.bytes));
    for session in &sessions {
        if count <= 1 || (count <= MAX_ARCHIVE_SESSIONS && total <= MAX_ARCHIVE_TOTAL_BYTES) {
            break;
        }
        for path in &session.paths {
            let _ = std::fs::remove_file(path);
        }
        count -= 1;
        total = total.saturating_sub(session.bytes);
    }
}

impl SerialDebugArchive {
    pub fn create(root_dir: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(root_dir)?;
        let (session_id, log_path, idx_path, log_writer, idx_writer) =
            Self::create_session_files(root_dir)?;
        // Prune after the new pair exists (same order as `prune_trace_files`):
        // the fresh files are the newest, so they can never be the ones trimmed.
        prune_serial_debug_archives(root_dir);
        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            session_id,
            log_path,
            idx_path,
            log_writer,
            idx_writer,
            next_offset: 0,
            next_line_no: 0,
            max_bytes: DEFAULT_SERIAL_DEBUG_ARCHIVE_MAX_BYTES,
            capped: false,
            pending_tx: Vec::new(),
            pending_rx: Vec::new(),
        })
    }

    /// Update the per-session byte cap (0 disables it). Raising it above the
    /// current size resumes archiving; lowering it stops at the next line.
    pub fn set_max_bytes(&mut self, max_bytes: u64) {
        self.max_bytes = max_bytes;
        if !self.would_exceed_cap(0) {
            self.capped = false;
        }
    }

    pub fn total_lines(&self) -> u64 {
        self.next_line_no
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.log_writer.flush()?;
        self.idx_writer.flush()?;
        let old_log_path = self.log_path.clone();
        let old_idx_path = self.idx_path.clone();
        let (session_id, log_path, idx_path, log_writer, idx_writer) =
            Self::create_session_files(&self.root_dir)?;
        self.session_id = session_id;
        self.log_path = log_path;
        self.idx_path = idx_path;
        self.log_writer = log_writer;
        self.idx_writer = idx_writer;
        self.next_offset = 0;
        self.next_line_no = 0;
        self.capped = false;
        self.pending_tx.clear();
        self.pending_rx.clear();
        let _ = std::fs::remove_file(old_log_path);
        let _ = std::fs::remove_file(old_idx_path);
        Ok(())
    }

    pub fn append_chunk(&mut self, chunk: &DebugChunk) -> std::io::Result<Vec<SerialDebugLine>> {
        let pending = match chunk.direction {
            Direction::Tx => &mut self.pending_tx,
            Direction::Rx => &mut self.pending_rx,
        };
        pending.extend_from_slice(&chunk.bytes);

        let mut decoded = Vec::new();
        loop {
            let raw_bytes = if let Some(newline_idx) = pending.iter().position(|&b| b == b'\n') {
                pending.drain(..=newline_idx).collect::<Vec<_>>()
            } else if pending.len() >= MAX_PENDING_SERIAL_DEBUG_LINE_BYTES {
                pending
                    .drain(..MAX_PENDING_SERIAL_DEBUG_LINE_BYTES)
                    .collect::<Vec<_>>()
            } else {
                break;
            };
            let text_end = raw_bytes
                .iter()
                .rposition(|&b| b != b'\n' && b != b'\r')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            let text = String::from_utf8_lossy(&raw_bytes[..text_end]).into_owned();
            let direction = log_direction(chunk.direction);
            // `raw_bytes` is deliberately dropped here: `text` above is already
            // derived from it, and carrying it into the archive JSON costs
            // ~268 B of `number[]` per 407 B line. Consumers that need bytes
            // (the hex view on a filter tab) re-encode `text`; the live view
            // never goes through the archive at all.
            decoded.push(SerialDebugLine {
                line_no: 0,
                ts_ms: chunk.ts_ms,
                direction,
                text,
                raw_bytes: None,
            });
        }

        let mut completed = Vec::with_capacity(decoded.len());
        for line in decoded {
            // `None` once the archive is capped — the drain above already
            // consumed the bytes, so the loop still converges.
            if let Some(line) = self.append_line(line)? {
                completed.push(line);
            }
        }
        if !completed.is_empty() {
            self.flush_writers()?;
        }

        Ok(completed)
    }

    /// Record a gap in the chunk stream for `direction`: whatever is buffered
    /// there is closed off as its own line, then a dropped-chunk sentinel `Sys`
    /// line is appended. Returns the lines written, newest last.
    ///
    /// Closing the buffer is the whole point. `append_chunk` only cuts a line at
    /// `\n`, so if a chunk is dropped mid-line the bytes before the gap and the
    /// bytes after it are concatenated into one line that looks perfectly
    /// ordinary — a log line the device never printed. Cutting here turns the
    /// loss into a visible line boundary with the sentinel right after it.
    pub fn append_gap(
        &mut self,
        direction: Direction,
        ts_ms: u64,
        dropped_bytes: u64,
    ) -> std::io::Result<Vec<SerialDebugLine>> {
        let mut written = Vec::new();
        if let Some(line) = self.close_pending_line(direction, ts_ms)? {
            written.push(line);
        }
        if let Some(line) = self.append_line(SerialDebugLine {
            line_no: 0,
            ts_ms,
            direction: LogDirection::Sys,
            // A sentinel, not prose — see `serial_debug_chunk_drop_sentinel`.
            text: serial_debug_chunk_drop_sentinel(dropped_bytes),
            raw_bytes: None,
        })? {
            written.push(line);
        }
        if !written.is_empty() {
            self.flush_writers()?;
        }
        Ok(written)
    }

    /// Close whatever is still buffered in *both* directions, for the end of a
    /// session. Returns the lines written, newest last.
    ///
    /// `append_chunk` only cuts a line at `\n`, so output the device never
    /// terminated — a `login: ` prompt, a progress bar, a bootloader prompt —
    /// stays in the pending buffer. Without this the port closes on top of it
    /// and those bytes exist nowhere: not in the live view, not in the archive,
    /// and therefore not in the export, the auto-save file, the filter tabs or
    /// the history window either. Empty buffers write nothing, so closing a
    /// quiet session adds no blank line.
    pub fn finalize_pending_lines(&mut self, ts_ms: u64) -> std::io::Result<Vec<SerialDebugLine>> {
        let mut written = Vec::new();
        for direction in [Direction::Tx, Direction::Rx] {
            if let Some(line) = self.close_pending_line(direction, ts_ms)? {
                written.push(line);
            }
        }
        if !written.is_empty() {
            self.flush_writers()?;
        }
        Ok(written)
    }

    /// Cut whatever is buffered for `direction` into a line of its own. `None`
    /// when nothing is buffered (or the archive is capped). Does not flush —
    /// callers batch that with the rest of what they write.
    fn close_pending_line(
        &mut self,
        direction: Direction,
        ts_ms: u64,
    ) -> std::io::Result<Option<SerialDebugLine>> {
        let pending = match direction {
            Direction::Tx => &mut self.pending_tx,
            Direction::Rx => &mut self.pending_rx,
        };
        let partial = std::mem::take(pending);
        if partial.is_empty() {
            return Ok(None);
        }
        let text_end = partial
            .iter()
            .rposition(|&b| b != b'\n' && b != b'\r')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        self.append_line(SerialDebugLine {
            line_no: 0,
            ts_ms,
            direction: log_direction(direction),
            text: String::from_utf8_lossy(&partial[..text_end]).into_owned(),
            raw_bytes: None,
        })
    }

    /// Returns `None` once the archive is capped (see [`Self::append_line`]).
    pub fn append_sys_line(
        &mut self,
        ts_ms: u64,
        text: String,
    ) -> std::io::Result<Option<SerialDebugLine>> {
        let line = self.append_line(SerialDebugLine {
            line_no: 0,
            ts_ms,
            direction: LogDirection::Sys,
            text,
            raw_bytes: None,
        })?;
        self.flush_writers()?;
        Ok(line)
    }

    pub fn read_line(&self, line_no: u64) -> std::io::Result<Option<SerialDebugLine>> {
        if line_no == 0 || line_no > self.total_lines() {
            return Ok(None);
        }
        let (offset, len) = self.read_index_entry(line_no)?;
        let mut file = File::open(&self.log_path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len as usize];
        file.read_exact(&mut buf)?;
        Ok(Some(serde_json::from_slice::<SerialDebugLine>(&buf)?))
    }

    pub fn read_lines(&self, line_nos: &[u64]) -> std::io::Result<Vec<SerialDebugLine>> {
        // `total_lines()` is read once, before any I/O. The archive is
        // append-only, so a run bounded by that snapshot is fully written and
        // flushed for the whole read — nothing a later append does can move
        // bytes the run already covers.
        read_archive_lines(
            &self.log_path,
            &self.idx_path,
            line_nos,
            Some(self.total_lines()),
        )
    }

    pub fn read_page(&self, start: u64, limit: u64) -> std::io::Result<SerialDebugSessionPage> {
        let total_lines = self.total_lines();
        let start = start.min(total_lines);
        let end = start.saturating_add(limit).min(total_lines);
        let line_nos = (start + 1..=end).collect::<Vec<_>>();
        Ok(SerialDebugSessionPage {
            total_lines,
            start,
            items: self.read_lines(&line_nos)?,
        })
    }

    pub fn read_line_range(
        &self,
        start_line_no: u64,
        limit: u64,
    ) -> std::io::Result<Vec<SerialDebugLine>> {
        if start_line_no == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let end = start_line_no
            .saturating_add(limit.saturating_sub(1))
            .min(self.total_lines());
        if start_line_no > end {
            return Ok(Vec::new());
        }
        let line_nos = (start_line_no..=end).collect::<Vec<_>>();
        self.read_lines(&line_nos)
    }

    pub fn snapshot_reader(&self) -> SerialDebugArchiveReader {
        SerialDebugArchiveReader {
            log_path: self.log_path.clone(),
            idx_path: self.idx_path.clone(),
        }
    }

    /// Append one line, or drop it once the archive is capped.
    ///
    /// Returns the archived line; `None` means nothing was written. The line
    /// that first crosses the cap is replaced by the cap sentinel, which *is*
    /// returned — the callers recognise it with
    /// [`serial_debug_archive_cap_limit_mib`] and notify the live view, so the
    /// cap surfaces there as well as in the export and the auto-save file
    /// rather than truncating silently.
    ///
    /// `next_line_no` is only incremented after the bytes are handed to the
    /// writers. Numbering a line that never reaches the `.idx` file would put a
    /// hole in it, and `(line_no - 1) * 16` would then read the wrong entry for
    /// every later line — silent corruption with no error anywhere.
    fn append_line(
        &mut self,
        mut line: SerialDebugLine,
    ) -> std::io::Result<Option<SerialDebugLine>> {
        if self.capped {
            return Ok(None);
        }
        line.line_no = self.next_line_no + 1;
        let encoded = serde_json::to_vec(&line)?;
        if self.would_exceed_cap(encoded.len() as u64) {
            self.capped = true;
            let ts_ms = line.ts_ms;
            return self.write_cap_notice(ts_ms).map(Some);
        }
        self.write_encoded_line(line, &encoded).map(Some)
    }

    /// Whether writing `encoded_len` more payload bytes would break the budget.
    /// Uses `next_offset` — the authoritative logical size — and never
    /// `metadata().len()`, which lags behind by whatever the `BufWriter` still
    /// holds (`flush_writers` only runs once per batch).
    fn would_exceed_cap(&self, encoded_len: u64) -> bool {
        if self.max_bytes == 0 {
            return false;
        }
        let budget = self
            .max_bytes
            .saturating_sub(ARCHIVE_CAP_NOTICE_HEADROOM_BYTES);
        self.next_offset.saturating_add(encoded_len + 1) > budget
    }

    fn write_cap_notice(&mut self, ts_ms: u64) -> std::io::Result<SerialDebugLine> {
        let line = SerialDebugLine {
            line_no: self.next_line_no + 1,
            ts_ms,
            direction: LogDirection::Sys,
            // A sentinel, not prose: the wording is a user-visible string and
            // belongs in the frontend i18n catalogue. See
            // `serial_debug_archive_cap_sentinel`.
            //
            // The UI floor is 16 MiB, so rounding up only ever shows on
            // deliberately tiny caps (tests).
            text: serial_debug_archive_cap_sentinel(self.max_bytes.div_ceil(1024 * 1024)),
            raw_bytes: None,
        };
        let encoded = serde_json::to_vec(&line)?;
        self.write_encoded_line(line, &encoded)
    }

    fn write_encoded_line(
        &mut self,
        line: SerialDebugLine,
        encoded: &[u8],
    ) -> std::io::Result<SerialDebugLine> {
        self.log_writer.write_all(encoded)?;
        self.log_writer.write_all(b"\n")?;
        self.idx_writer.write_all(&self.next_offset.to_le_bytes())?;
        self.idx_writer
            .write_all(&(encoded.len() as u64).to_le_bytes())?;
        self.next_offset += encoded.len() as u64 + 1;
        self.next_line_no += 1;
        Ok(line)
    }

    fn flush_writers(&mut self) -> std::io::Result<()> {
        self.log_writer.flush()?;
        self.idx_writer.flush()?;
        Ok(())
    }

    fn read_index_entry(&self, line_no: u64) -> std::io::Result<(u64, u64)> {
        let mut idx_file = File::open(&self.idx_path)?;
        self.read_index_entry_with_file(line_no, &mut idx_file)
    }

    fn read_index_entry_with_file(
        &self,
        line_no: u64,
        idx_file: &mut File,
    ) -> std::io::Result<(u64, u64)> {
        idx_file.seek(SeekFrom::Start((line_no - 1) * ARCHIVE_INDEX_ENTRY_BYTES))?;
        let mut offset_buf = [0u8; 8];
        let mut len_buf = [0u8; 8];
        idx_file.read_exact(&mut offset_buf)?;
        idx_file.read_exact(&mut len_buf)?;
        Ok((u64::from_le_bytes(offset_buf), u64::from_le_bytes(len_buf)))
    }

    fn create_session_files(
        root_dir: &std::path::Path,
    ) -> std::io::Result<SerialDebugSessionFiles> {
        let session_id = next_serial_debug_session_id(now_ms(), std::process::id());
        let log_path = root_dir.join(format!("serial-debug-session-{session_id}.ndjson"));
        let idx_path = root_dir.join(format!("serial-debug-session-{session_id}.idx"));
        let log_writer = std::io::BufWriter::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&log_path)?,
        );
        let idx_writer = std::io::BufWriter::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&idx_path)?,
        );
        Ok((session_id, log_path, idx_path, log_writer, idx_writer))
    }
}

impl SerialDebugArchiveReader {
    pub fn read_lines(&self, line_nos: &[u64]) -> std::io::Result<Vec<SerialDebugLine>> {
        // No `total_lines` bound here: the reader is a path snapshot with no
        // view of the writer's counter, so a line number past the end still
        // fails the `.idx` read as it always has. Callers bound the range with
        // the total they snapshotted alongside the reader.
        read_archive_lines(&self.log_path, &self.idx_path, line_nos, None)
    }

    pub fn read_line_range(
        &self,
        start_line_no: u64,
        limit: u64,
    ) -> std::io::Result<Vec<SerialDebugLine>> {
        if start_line_no == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let line_nos = (start_line_no..start_line_no.saturating_add(limit)).collect::<Vec<_>>();
        self.read_lines(&line_nos)
    }
}

/// Read the archived lines named by `line_nos`, in that order, skipping line 0
/// and — when `max_line_no` is set — anything past the end of the archive.
///
/// Line numbers that ascend by exactly one are grouped into a *run* and fetched
/// by [`read_line_run`] with a handful of syscalls instead of four per line;
/// that is the whole point, since paging (`read_page` / `read_line_range`) asks
/// for nothing but one long run. Sparse input — a filter's match list — simply
/// degenerates to runs of length one, which cost what the per-line path always
/// cost.
fn read_archive_lines(
    log_path: &std::path::Path,
    idx_path: &std::path::Path,
    line_nos: &[u64],
    max_line_no: Option<u64>,
) -> std::io::Result<Vec<SerialDebugLine>> {
    let mut idx_file = File::open(idx_path)?;
    let mut log_file = File::open(log_path)?;
    let mut items = Vec::with_capacity(line_nos.len());
    let in_range = |line_no: u64| line_no != 0 && max_line_no.is_none_or(|max| line_no <= max);

    let mut i = 0;
    while i < line_nos.len() {
        let first = line_nos[i];
        if !in_range(first) {
            i += 1;
            continue;
        }
        let mut run = 1;
        while i + run < line_nos.len() {
            let next = first.saturating_add(run as u64);
            if line_nos[i + run] != next || !in_range(next) {
                break;
            }
            run += 1;
        }
        read_line_run(&mut idx_file, &mut log_file, first, run, &mut items)?;
        i += run;
    }
    Ok(items)
}

/// Append `count` consecutive archived lines starting at `first_line_no`
/// (1-based) to `out`.
///
/// The `.idx` entries of consecutive lines are themselves consecutive
/// (`(line_no - 1) * 16`), and the `.ndjson` is append-only — every line is
/// written at `next_offset` and advances it by `len + 1` — so the payloads of
/// consecutive lines form one contiguous byte span with a single `\n` between
/// them. That holds for every append path (`append_chunk`, `append_gap`,
/// `append_sys_line`, the cap notice), because they all funnel through
/// `write_encoded_line`, and the `stopWriting` cap never rewrites or renumbers.
/// So one seek+read fetches the whole index slice and one more fetches the whole
/// payload span: 4 syscalls per batch instead of 4 per line.
fn read_line_run(
    idx_file: &mut File,
    log_file: &mut File,
    first_line_no: u64,
    count: usize,
    out: &mut Vec<SerialDebugLine>,
) -> std::io::Result<()> {
    let mut done = 0;
    while done < count {
        let batch = (count - done).min(ARCHIVE_READ_INDEX_BATCH_LINES);
        let batch_first = first_line_no + done as u64;
        idx_file.seek(SeekFrom::Start(
            (batch_first - 1) * ARCHIVE_INDEX_ENTRY_BYTES,
        ))?;
        let mut idx_buf = vec![0u8; batch * ARCHIVE_INDEX_ENTRY_BYTES as usize];
        idx_file.read_exact(&mut idx_buf)?;

        let entries = idx_buf
            .chunks_exact(ARCHIVE_INDEX_ENTRY_BYTES as usize)
            .map(|entry| {
                let mut offset = [0u8; 8];
                let mut len = [0u8; 8];
                offset.copy_from_slice(&entry[..8]);
                len.copy_from_slice(&entry[8..]);
                (u64::from_le_bytes(offset), u64::from_le_bytes(len))
            })
            .collect::<Vec<_>>();

        let mut i = 0;
        while i < entries.len() {
            // Grow the span while it stays inside the byte budget; `last` never
            // stays below `i`, so a single oversized line still makes progress.
            let (span_start, first_len) = entries[i];
            let mut last = i;
            let mut span_end = span_start + first_len;
            while last + 1 < entries.len() {
                let (next_offset, next_len) = entries[last + 1];
                let next_end = next_offset + next_len;
                if next_end - span_start > ARCHIVE_READ_SPAN_BUDGET_BYTES {
                    break;
                }
                last += 1;
                span_end = next_end;
            }
            let mut span = vec![0u8; (span_end - span_start) as usize];
            log_file.seek(SeekFrom::Start(span_start))?;
            log_file.read_exact(&mut span)?;
            for &(offset, len) in &entries[i..=last] {
                let from = (offset - span_start) as usize;
                out.push(serde_json::from_slice::<SerialDebugLine>(
                    &span[from..from + len as usize],
                )?);
            }
            i = last + 1;
        }
        done += batch;
    }
    Ok(())
}

impl SerialDebugFilterIndex {
    pub fn add_filter(
        &mut self,
        keyword: String,
        use_regex: bool,
        color: String,
        snapshot_total_lines: u64,
    ) -> Result<SerialDebugFilterDefinition, String> {
        let trimmed = keyword.trim();
        if trimmed.is_empty() {
            return Err("keyword must not be empty".into());
        }
        if self
            .filters
            .values()
            .any(|entry| entry.def.keyword == trimmed && entry.def.use_regex == use_regex)
        {
            return Err("duplicate filter".into());
        }

        let regex = if use_regex {
            Some(regex::Regex::new(trimmed).map_err(|e| e.to_string())?)
        } else {
            None
        };

        let def = SerialDebugFilterDefinition {
            id: format!("filter-{}", self.next_filter_id),
            keyword: trimmed.to_string(),
            use_regex,
            color,
        };
        let match_idx_path = self.filter_match_index_path(&def.id);
        let match_idx_writer =
            open_filter_match_index_writer(&match_idx_path).map_err(|e| e.to_string())?;
        self.next_filter_id += 1;
        self.filters.insert(
            def.id.clone(),
            SerialDebugFilterEntry {
                def: def.clone(),
                status: SerialDebugFilterStatus::Pending,
                snapshot_total_lines,
                scanned_until_line_no: 0,
                total_matches: 0,
                error: None,
                match_idx_path,
                match_idx_writer,
                pending_live_match_idx_path: None,
                pending_live_match_idx_writer: None,
                pending_live_matches: 0,
                pending_live_until_line_no: 0,
                regex,
            },
        );
        Ok(def)
    }

    pub fn remove_filter(&mut self, filter_id: &str) -> bool {
        if let Some(mut entry) = self.filters.remove(filter_id) {
            let _ = entry.flush_match_index();
            let _ = entry.flush_pending_live_match_index();
            let _ = std::fs::remove_file(entry.match_idx_path);
            if let Some(path) = entry.pending_live_match_idx_path {
                let _ = std::fs::remove_file(path);
            }
            true
        } else {
            false
        }
    }

    pub fn reset_for_new_session(&mut self) -> Vec<SerialDebugFilterStats> {
        let mut stats = Vec::with_capacity(self.filters.len());
        for entry in self.filters.values_mut() {
            entry.snapshot_total_lines = 0;
            entry.scanned_until_line_no = 0;
            entry.total_matches = 0;
            entry.error = None;
            match entry.reset_match_index() {
                Ok(()) => {
                    let _ = entry.reset_pending_live_match_index(None);
                    entry.status = SerialDebugFilterStatus::Complete;
                }
                Err(e) => {
                    entry.status = SerialDebugFilterStatus::Failed;
                    entry.error = Some(e.to_string());
                }
            }
            stats.push(entry.stats());
        }
        stats.sort_by(|a, b| a.filter_id.cmp(&b.filter_id));
        stats
    }

    pub fn ingest_completed_lines(
        &mut self,
        lines: &[SerialDebugLine],
    ) -> Result<Vec<SerialDebugFilterStats>, String> {
        let mut changed_filter_ids = std::collections::HashSet::new();
        for line in lines {
            for entry in self.filters.values_mut() {
                if line.line_no <= entry.snapshot_total_lines {
                    continue;
                }
                if entry.matches(&line.text) {
                    if entry.is_backfilling() {
                        entry
                            .append_pending_live_match_line_no(line.line_no)
                            .map_err(|e| e.to_string())?;
                    } else {
                        entry
                            .append_match_line_no(line.line_no)
                            .map_err(|e| e.to_string())?;
                        entry.total_matches += 1;
                        changed_filter_ids.insert(entry.def.id.clone());
                    }
                }
            }
        }
        let mut changed_filter_ids = changed_filter_ids.into_iter().collect::<Vec<_>>();
        changed_filter_ids.sort();
        for filter_id in &changed_filter_ids {
            if let Some(entry) = self.filters.get_mut(filter_id) {
                entry.flush_match_index().map_err(|e| e.to_string())?;
            }
        }
        Ok(changed_filter_ids
            .into_iter()
            .filter_map(|filter_id| self.stats(&filter_id))
            .collect::<Vec<_>>())
    }

    pub fn start_backfill(&mut self, filter_id: &str) -> std::io::Result<SerialDebugFilterStats> {
        let pending_live_match_idx_path = self.pending_live_filter_match_index_path(filter_id);
        let entry = self
            .filters
            .get_mut(filter_id)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found"))?;
        let pre_backfill_live_match_count = entry.total_matches;
        let pre_backfill_live_until_line_no = if pre_backfill_live_match_count > 0 {
            entry
                .read_match_line_nos(pre_backfill_live_match_count - 1, 1)?
                .into_iter()
                .next()
                .unwrap_or(0)
        } else {
            0
        };

        {
            let mut pending_writer = open_filter_match_index_writer(&pending_live_match_idx_path)?;
            if pre_backfill_live_match_count > 0 {
                let mut existing_main_file = File::open(&entry.match_idx_path)?;
                std::io::copy(&mut existing_main_file, &mut pending_writer)?;
                pending_writer.flush()?;
            }
            entry.pending_live_match_idx_writer = Some(pending_writer);
        }

        entry.match_idx_writer = open_filter_match_index_writer(&entry.match_idx_path)?;
        entry.status = SerialDebugFilterStatus::Backfilling;
        entry.error = None;
        entry.scanned_until_line_no = 0;
        entry.total_matches = 0;
        entry.pending_live_matches = pre_backfill_live_match_count;
        entry.pending_live_until_line_no = pre_backfill_live_until_line_no;
        entry.pending_live_match_idx_path = Some(pending_live_match_idx_path);
        Ok(entry.stats())
    }

    pub fn finish_backfill_from_file(
        &mut self,
        filter_id: &str,
        historical_idx_path: &std::path::Path,
        historical_match_count: u64,
        historical_scanned_until_line_no: u64,
    ) -> std::io::Result<SerialDebugFilterStats> {
        let entry = self
            .filters
            .get_mut(filter_id)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found"))?;
        entry.flush_match_index()?;
        entry.flush_pending_live_match_index()?;
        {
            entry.match_idx_writer = open_filter_match_index_writer(&entry.match_idx_path)?;
            if historical_idx_path.exists() {
                let mut historical_file = File::open(historical_idx_path)?;
                std::io::copy(&mut historical_file, &mut entry.match_idx_writer)?;
            }
            if let Some(pending_path) = &entry.pending_live_match_idx_path {
                if pending_path.exists() {
                    let mut pending_file = File::open(pending_path)?;
                    std::io::copy(&mut pending_file, &mut entry.match_idx_writer)?;
                }
            }
            entry.match_idx_writer.flush()?;
        }
        entry.match_idx_writer = open_filter_match_index_append_writer(&entry.match_idx_path)?;
        entry.total_matches = historical_match_count + entry.pending_live_matches;
        entry.snapshot_total_lines =
            historical_scanned_until_line_no.max(entry.pending_live_until_line_no);
        entry.scanned_until_line_no = entry.snapshot_total_lines;
        entry.status = SerialDebugFilterStatus::Complete;
        entry.error = None;
        entry.reset_pending_live_match_index(None)?;
        Ok(entry.stats())
    }

    pub fn backfill_snapshot(&self, filter_id: &str) -> Option<SerialDebugFilterBackfillSnapshot> {
        self.filters
            .get(filter_id)
            .map(|entry| SerialDebugFilterBackfillSnapshot {
                filter_id: entry.def.id.clone(),
                keyword: entry.def.keyword.clone(),
                use_regex: entry.def.use_regex,
                snapshot_total_lines: entry.snapshot_total_lines,
            })
    }

    pub fn fail_backfill(
        &mut self,
        filter_id: &str,
        error: String,
    ) -> std::io::Result<SerialDebugFilterStats> {
        let entry = self
            .filters
            .get_mut(filter_id)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found"))?;
        entry.status = SerialDebugFilterStatus::Failed;
        entry.error = Some(error);
        entry.reset_pending_live_match_index(None)?;
        Ok(entry.stats())
    }

    pub fn backfill_filter(
        &mut self,
        filter_id: &str,
        archive: &SerialDebugArchive,
    ) -> std::io::Result<SerialDebugFilterStats> {
        let snapshot_total_lines = {
            let entry = self.filters.get(filter_id).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found")
            })?;
            if matches!(entry.status, SerialDebugFilterStatus::Complete) {
                return Ok(entry.stats());
            }
            entry.snapshot_total_lines
        };

        let _ = self.start_backfill(filter_id)?;
        let historical_idx_path = self
            .root_dir
            .join(format!("serial-debug-filter-{filter_id}.historical.idx"));
        let mut historical_writer = open_filter_match_index_writer(&historical_idx_path)?;
        let mut historical_match_count = 0u64;
        let mut scanned_until_line_no = 0u64;
        let backfill_result = (|| -> std::io::Result<SerialDebugFilterStats> {
            let regex = self
                .filters
                .get(filter_id)
                .and_then(|entry| entry.regex.clone());
            let keyword = self
                .filters
                .get(filter_id)
                .map(|entry| entry.def.keyword.clone())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found")
                })?;
            let mut start_line_no = 1;
            while start_line_no <= snapshot_total_lines {
                let remaining = snapshot_total_lines
                    .saturating_sub(start_line_no)
                    .saturating_add(1);
                let lines = archive.read_line_range(
                    start_line_no,
                    remaining.min(FILTER_BACKFILL_READ_BATCH_LINES),
                )?;
                if lines.is_empty() {
                    break;
                }
                for line in lines {
                    let is_match = match &regex {
                        Some(re) => re.is_match(&line.text),
                        None => line.text.contains(&keyword),
                    };
                    if is_match {
                        historical_writer.write_all(&line.line_no.to_le_bytes())?;
                        historical_match_count += 1;
                    }
                    scanned_until_line_no = line.line_no;
                }
                start_line_no = scanned_until_line_no.saturating_add(1);
            }
            historical_writer.flush()?;
            self.finish_backfill_from_file(
                filter_id,
                &historical_idx_path,
                historical_match_count,
                scanned_until_line_no,
            )
        })();
        let _ = std::fs::remove_file(&historical_idx_path);
        match backfill_result {
            Ok(stats) => Ok(stats),
            Err(e) => {
                let _ = self.fail_backfill(filter_id, e.to_string());
                Err(e)
            }
        }
    }

    pub fn read_match_page(
        &self,
        filter_id: &str,
        start: u64,
        limit: u64,
        archive: &SerialDebugArchive,
    ) -> std::io::Result<SerialDebugFilterPage> {
        let entry = self
            .filters
            .get(filter_id)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "filter not found"))?;
        let start = start.min(entry.total_matches);
        let line_nos = entry.read_match_line_nos(start, limit)?;
        let items = archive.read_lines(&line_nos)?;
        Ok(SerialDebugFilterPage {
            filter_id: filter_id.to_string(),
            total_matches: entry.total_matches,
            start,
            items,
        })
    }

    pub fn stats(&self, filter_id: &str) -> Option<SerialDebugFilterStats> {
        self.filters
            .get(filter_id)
            .map(SerialDebugFilterEntry::stats)
    }

    pub fn definition(&self, filter_id: &str) -> Option<SerialDebugFilterDefinition> {
        self.filters.get(filter_id).map(|entry| entry.def.clone())
    }

    pub fn list_filters(&self) -> Vec<(SerialDebugFilterDefinition, SerialDebugFilterStats)> {
        let mut items = self
            .filters
            .values()
            .map(|entry| (entry.def.clone(), entry.stats()))
            .collect::<Vec<_>>();
        items.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        items
    }
}

pub fn serial_debug_scan_filter_matches(
    snapshot: &SerialDebugFilterBackfillSnapshot,
    archive_reader: &SerialDebugArchiveReader,
    historical_idx_path: &std::path::Path,
) -> std::io::Result<(u64, u64)> {
    let regex =
        if snapshot.use_regex {
            Some(regex::Regex::new(&snapshot.keyword).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
            })?)
        } else {
            None
        };
    let mut historical_writer = open_filter_match_index_writer(historical_idx_path)?;
    let mut historical_match_count = 0u64;
    let mut scanned_until_line_no = 0u64;
    let mut start_line_no = 1;
    while start_line_no <= snapshot.snapshot_total_lines {
        let remaining = snapshot
            .snapshot_total_lines
            .saturating_sub(start_line_no)
            .saturating_add(1);
        let lines = archive_reader.read_line_range(
            start_line_no,
            remaining.min(FILTER_BACKFILL_READ_BATCH_LINES),
        )?;
        if lines.is_empty() {
            break;
        }
        for line in lines {
            let is_match = match &regex {
                Some(re) => re.is_match(&line.text),
                None => line.text.contains(&snapshot.keyword),
            };
            if is_match {
                historical_writer.write_all(&line.line_no.to_le_bytes())?;
                historical_match_count += 1;
            }
            scanned_until_line_no = line.line_no;
        }
        start_line_no = scanned_until_line_no.saturating_add(1);
    }
    historical_writer.flush()?;
    Ok((historical_match_count, scanned_until_line_no))
}

pub fn serial_debug_finish_backfill_if_current(
    generation: &SerialDebugGeneration,
    expected_generation: u64,
    filters: &mut SerialDebugFilterIndex,
    filter_id: &str,
    historical_idx_path: &std::path::Path,
    historical_match_count: u64,
    historical_scanned_until_line_no: u64,
) -> std::io::Result<Option<SerialDebugFilterStats>> {
    if !generation.is_current(expected_generation) {
        return Ok(None);
    }
    filters
        .finish_backfill_from_file(
            filter_id,
            historical_idx_path,
            historical_match_count,
            historical_scanned_until_line_no,
        )
        .map(Some)
}

pub fn serial_debug_fail_backfill_if_current(
    generation: &SerialDebugGeneration,
    expected_generation: u64,
    filters: &mut SerialDebugFilterIndex,
    filter_id: &str,
    error: String,
) -> std::io::Result<Option<SerialDebugFilterStats>> {
    if !generation.is_current(expected_generation) {
        return Ok(None);
    }
    filters.fail_backfill(filter_id, error).map(Some)
}

impl SerialDebugFilterEntry {
    fn is_backfilling(&self) -> bool {
        matches!(self.status, SerialDebugFilterStatus::Backfilling)
    }

    fn matches(&self, text: &str) -> bool {
        match &self.regex {
            Some(re) => re.is_match(text),
            None => text.contains(&self.def.keyword),
        }
    }

    fn append_match_line_no(&mut self, line_no: u64) -> std::io::Result<()> {
        self.match_idx_writer.write_all(&line_no.to_le_bytes())
    }

    fn flush_match_index(&mut self) -> std::io::Result<()> {
        self.match_idx_writer.flush()
    }

    fn reset_match_index(&mut self) -> std::io::Result<()> {
        self.match_idx_writer.flush()?;
        self.match_idx_writer = open_filter_match_index_writer(&self.match_idx_path)?;
        Ok(())
    }

    fn append_pending_live_match_line_no(&mut self, line_no: u64) -> std::io::Result<()> {
        if let Some(writer) = self.pending_live_match_idx_writer.as_mut() {
            writer.write_all(&line_no.to_le_bytes())?;
            self.pending_live_matches += 1;
            self.pending_live_until_line_no = line_no;
        }
        Ok(())
    }

    fn flush_pending_live_match_index(&mut self) -> std::io::Result<()> {
        if let Some(writer) = self.pending_live_match_idx_writer.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }

    fn reset_pending_live_match_index(
        &mut self,
        path: Option<std::path::PathBuf>,
    ) -> std::io::Result<()> {
        self.flush_pending_live_match_index()?;
        if let Some(existing_path) = &self.pending_live_match_idx_path {
            let _ = std::fs::remove_file(existing_path);
        }
        self.pending_live_match_idx_path = path;
        self.pending_live_match_idx_writer = None;
        self.pending_live_matches = 0;
        self.pending_live_until_line_no = 0;
        Ok(())
    }

    fn read_match_line_nos(&self, start: u64, limit: u64) -> std::io::Result<Vec<u64>> {
        if start >= self.total_matches || limit == 0 {
            return Ok(Vec::new());
        }
        let count = self.total_matches.saturating_sub(start).min(limit) as usize;
        let mut file = File::open(&self.match_idx_path)?;
        file.seek(SeekFrom::Start(start * FILTER_MATCH_INDEX_ENTRY_BYTES))?;
        let mut line_nos = Vec::with_capacity(count);
        for _ in 0..count {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
            line_nos.push(u64::from_le_bytes(buf));
        }
        Ok(line_nos)
    }

    fn stats(&self) -> SerialDebugFilterStats {
        SerialDebugFilterStats {
            filter_id: self.def.id.clone(),
            status: self.status,
            scanned_until_line_no: self.scanned_until_line_no,
            total_lines_snapshot: self.snapshot_total_lines,
            total_matches: self.total_matches,
            error: self.error.clone(),
        }
    }
}

pub struct SerialDebugSession {
    cfg: DebugConfig,
    /// Separate clone of the port used exclusively for writes.
    /// The read loop owns the other half; keeping them separate avoids
    /// holding a mutex during blocking reads, which would stall writes.
    write_port: Arc<Mutex<Box<dyn serialport::SerialPort>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Default for SerialDebugChunkBatchBuffer {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The clock the serial-debug engine timestamps with, for hosts that have to
/// stamp drop reports and gap lines with the same reading.
pub fn serial_debug_now_ms() -> u64 {
    now_ms()
}

fn next_serial_debug_session_id(now_ms: u64, pid: u32) -> String {
    let seq = SERIAL_DEBUG_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{now_ms}-{pid}-{seq}")
}

#[cfg(test)]
fn next_serial_debug_session_id_for_tests(now_ms: u64, pid: u32) -> String {
    next_serial_debug_session_id(now_ms, pid)
}

fn map_data_bits(v: DataBits) -> serialport::DataBits {
    match v {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(v: Parity) -> serialport::Parity {
    match v {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_stop_bits(v: StopBits) -> serialport::StopBits {
    // serialport crate does not support 1.5 natively on all platforms.
    // Map 1.5 to One with a log warning; the OS driver may still honor it.
    match v {
        StopBits::One | StopBits::OnePointFive => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    }
}

impl SerialDebugSession {
    pub fn open(
        cfg: DebugConfig,
        on_chunk: ChunkCallback,
        on_disconnect: DisconnectCallback,
    ) -> Result<Self, FlashError> {
        if matches!(cfg.stop_bits, StopBits::OnePointFive) {
            log::warn!(
                "[SerialDebug] stop_bits=1.5 requested; the serialport crate does not support \
                 it directly — falling back to 1 stop bit. OS drivers may differ."
            );
        }
        let builder = serialport::new(&cfg.port, cfg.baud_rate)
            .data_bits(map_data_bits(cfg.data_bits))
            .parity(map_parity(cfg.parity))
            .stop_bits(map_stop_bits(cfg.stop_bits))
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(50));
        let read_port = builder.open().map_err(|e| {
            FlashError::Io(std::io::Error::other(format!(
                "open {} failed: {}",
                cfg.port, e
            )))
        })?;
        let write_port = read_port.try_clone().map_err(|e| {
            FlashError::Io(std::io::Error::other(format!(
                "clone port handle failed: {}",
                e
            )))
        })?;

        let write_port = Arc::new(Mutex::new(write_port));
        let stop = Arc::new(AtomicBool::new(false));

        let reader = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name(format!("serial-debug-read:{}", cfg.port))
                .spawn(move || {
                    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        read_loop(read_port, stop, on_chunk, on_disconnect);
                    }));
                    if let Err(payload) = run {
                        log::error!("[SerialDebug] reader thread panicked: {:?}", payload);
                    }
                })
                .map_err(|e| {
                    FlashError::Io(std::io::Error::other(format!(
                        "spawn reader thread failed: {}",
                        e
                    )))
                })?
        };

        log::info!(
            "[SerialDebug] opened {} @ {} {:?}/{:?}/{:?}",
            cfg.port,
            cfg.baud_rate,
            cfg.data_bits,
            cfg.parity,
            cfg.stop_bits
        );

        Ok(Self {
            cfg,
            write_port,
            stop,
            reader: Some(reader),
        })
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), FlashError> {
        let mut guard = self
            .write_port
            .lock()
            .map_err(|_| FlashError::Io(std::io::Error::other("serial debug mutex poisoned")))?;
        guard.write_all(bytes).map_err(FlashError::Io)?;
        Ok(())
    }

    pub fn device_reset(&self, chip_id: &str) -> Result<(), FlashError> {
        let mut guard = self
            .write_port
            .lock()
            .map_err(|_| FlashError::Io(std::io::Error::other("serial debug mutex poisoned")))?;
        crate::serial::device_reset_serial_port(&self.cfg.port, &mut **guard, chip_id)
    }

    pub fn close(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        log::info!("[SerialDebug] closed {}", self.cfg.port);
    }

    pub fn is_open(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &DebugConfig {
        &self.cfg
    }
}

impl SerialDebugChunkBatchBuffer {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            pending_bytes: 0,
            first_chunk_at: None,
        }
    }

    pub fn push(&mut self, chunk: DebugChunk) {
        if self.chunks.is_empty() {
            self.first_chunk_at = Some(Instant::now());
        }
        self.pending_bytes += chunk.bytes.len();
        self.chunks.push(chunk);
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn should_flush_bytes(&self, max_bytes: usize) -> bool {
        self.pending_bytes >= max_bytes
    }

    pub fn should_flush_elapsed(&self, max_age: Duration) -> bool {
        self.first_chunk_at
            .is_some_and(|started| started.elapsed() >= max_age)
    }

    pub fn take(&mut self) -> Vec<DebugChunk> {
        self.pending_bytes = 0;
        self.first_chunk_at = None;
        std::mem::take(&mut self.chunks)
    }
}

impl Drop for SerialDebugSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}

fn read_loop(
    mut port: Box<dyn serialport::SerialPort>,
    stop: Arc<AtomicBool>,
    on_chunk: ChunkCallback,
    on_disconnect: DisconnectCallback,
) {
    let mut buf = [0u8; 4096];
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let read_result = port.read(&mut buf);
        match read_result {
            Ok(0) => continue,
            Ok(n) => {
                let chunk = DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: now_ms(),
                    bytes: buf[..n].to_vec(),
                };
                on_chunk(chunk);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e)
                if e.kind() == std::io::ErrorKind::BrokenPipe
                    || e.kind() == std::io::ErrorKind::NotFound =>
            {
                log::warn!(
                    "[SerialDebug] reader IO error: {} ({:?}) — disconnecting",
                    e,
                    e.kind()
                );
                on_disconnect(format!("{}", e));
                return;
            }
            Err(e) => {
                log::error!(
                    "[SerialDebug] reader unexpected error: {} ({:?}) — disconnecting",
                    e,
                    e.kind()
                );
                on_disconnect(format!("{}", e));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_config_round_trips_json_camel_case() {
        let cfg = DebugConfig {
            port: "/dev/ttyUSB0".into(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"baudRate\":115200"));
        assert!(json.contains("\"dataBits\":\"eight\""));
        assert!(json.contains("\"parity\":\"none\""));
        assert!(json.contains("\"stopBits\":\"one\""));
        let back: DebugConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn debug_chunk_serializes_direction_lowercase() {
        let chunk = DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1_700_000_000_000,
            bytes: vec![0x41, 0x42],
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"direction\":\"rx\""));
        assert!(json.contains("\"tsMs\":1700000000000"));
        assert!(json.contains("\"bytes\":[65,66]"));
    }

    // Loopback integration — only runs on Linux/macOS where serialport exposes pair().
    // If the `pair()` call fails (CI without PTY support), the test skips via an early return.
    //
    // Note: in serialport 4.x, `TTYPort::pair()` returns (master, slave). The master has no
    // port_name (it is the /dev/ptmx fd), while the slave exposes its /dev/pts/N path.
    // The session opens the slave by name (second independent fd) and we write on the
    // master handle we keep alive.
    #[test]
    fn stop_bits_one_point_five_serializes_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&StopBits::OnePointFive).unwrap(),
            "\"onePointFive\""
        );
        let back: StopBits = serde_json::from_str("\"onePointFive\"").unwrap();
        assert_eq!(back, StopBits::OnePointFive);
    }

    #[test]
    fn chunk_batch_buffer_tracks_bytes_and_resets_on_take() {
        let mut buffer = SerialDebugChunkBatchBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.pending_bytes(), 0);

        buffer.push(DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1,
            bytes: vec![1, 2, 3],
        });
        assert!(!buffer.is_empty());
        assert_eq!(buffer.pending_bytes(), 3);
        assert!(!buffer.should_flush_bytes(4));

        buffer.push(DebugChunk {
            direction: Direction::Tx,
            ts_ms: 2,
            bytes: vec![4, 5],
        });
        assert_eq!(buffer.pending_bytes(), 5);
        assert!(buffer.should_flush_bytes(5));

        let batch = buffer.take();
        assert_eq!(batch.len(), 2);
        assert!(buffer.is_empty());
        assert_eq!(buffer.pending_bytes(), 0);
    }

    #[test]
    fn chunk_batch_buffer_tracks_elapsed_since_first_chunk() {
        let mut buffer = SerialDebugChunkBatchBuffer::new();
        assert!(!buffer.should_flush_elapsed(Duration::ZERO));

        buffer.push(DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1,
            bytes: vec![1],
        });
        assert!(buffer.should_flush_elapsed(Duration::ZERO));

        let _ = buffer.take();
        assert!(!buffer.should_flush_elapsed(Duration::ZERO));
    }

    #[cfg(unix)]
    #[test]
    fn write_is_observed_on_the_paired_end_and_close_stops_reader() {
        use serialport::SerialPort;
        use std::io::Write;
        use std::sync::mpsc::channel;

        let Ok((mut master, slave)) = serialport::TTYPort::pair() else {
            eprintln!("serialport::TTYPort::pair() unavailable on this host; skipping");
            return;
        };
        let Some(slave_name) = slave.name() else {
            eprintln!("pty slave has no name on this host; skipping");
            return;
        };
        // Drop the slave handle so SerialDebugSession can open the path itself.
        drop(slave);

        let (tx_chunk, rx_chunk) = channel::<DebugChunk>();
        let (tx_disc, rx_disc) = channel::<String>();

        let cfg = DebugConfig {
            port: slave_name.clone(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        };
        let session = SerialDebugSession::open(
            cfg,
            Box::new(move |c| {
                let _ = tx_chunk.send(c);
            }),
            Box::new(move |r| {
                let _ = tx_disc.send(r);
            }),
        )
        .expect("session open");

        // Write on the master fd; the session's read loop on the slave should produce a chunk.
        master.write_all(b"ping\n").expect("write master");
        master.flush().expect("flush master");

        let mut accumulated: Vec<u8> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        while accumulated.len() < b"ping\n".len() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                panic!("timed out waiting for full payload; got {:?}", accumulated);
            }
            let chunk = rx_chunk
                .recv_timeout(remaining)
                .expect("expected chunk within deadline");
            assert_eq!(chunk.direction, Direction::Rx);
            accumulated.extend_from_slice(&chunk.bytes);
        }
        assert_eq!(&accumulated[..b"ping\n".len()], b"ping\n");

        session.close();
        assert!(
            rx_disc.try_recv().is_err(),
            "close() should not trigger on_disconnect"
        );
    }

    #[cfg(unix)]
    #[test]
    fn is_open_reports_true_until_closed_and_config_is_preserved() {
        let Ok((master, slave)) = serialport::TTYPort::pair() else {
            eprintln!("serialport::TTYPort::pair() unavailable on this host; skipping");
            return;
        };
        use serialport::SerialPort;
        let Some(slave_name) = slave.name() else {
            eprintln!("pty slave has no name on this host; skipping");
            return;
        };
        drop(slave);

        let cfg = DebugConfig {
            port: slave_name,
            baud_rate: 9600,
            data_bits: DataBits::Seven,
            parity: Parity::Even,
            stop_bits: StopBits::Two,
        };
        let session = SerialDebugSession::open(cfg.clone(), Box::new(|_| {}), Box::new(|_| {}))
            .expect("session open");

        assert!(session.is_open(), "session should report open before close");
        // The stored config must match what we opened with.
        assert_eq!(session.config(), &cfg);

        // Keep the master alive until after we inspect state.
        drop(master);
        session.close();
    }

    #[cfg(unix)]
    #[test]
    fn dropping_peer_end_triggers_disconnect_callback() {
        use std::sync::mpsc::channel;

        let Ok((master, slave)) = serialport::TTYPort::pair() else {
            eprintln!("serialport::TTYPort::pair() unavailable on this host; skipping");
            return;
        };
        use serialport::SerialPort;
        let Some(slave_name) = slave.name() else {
            eprintln!("pty slave has no name on this host; skipping");
            return;
        };
        drop(slave);

        let (tx_disc, rx_disc) = channel::<String>();
        let cfg = DebugConfig {
            port: slave_name,
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        };
        let session = SerialDebugSession::open(
            cfg,
            Box::new(|_| {}),
            Box::new(move |r| {
                let _ = tx_disc.send(r);
            }),
        )
        .expect("session open");

        // Closing the master pty end makes reads on the slave fail; the read loop
        // should surface that through on_disconnect. Some platforms only report a
        // timeout (no error) — in that case the callback never fires and we accept
        // either outcome rather than asserting a host-specific behavior.
        drop(master);

        match rx_disc.recv_timeout(Duration::from_millis(1500)) {
            Ok(reason) => assert!(!reason.is_empty(), "disconnect reason should be non-empty"),
            Err(_) => eprintln!(
                "peer drop did not surface as a read error on this host; accepted as timeout-only"
            ),
        }

        session.close();
    }

    #[test]
    fn data_bits_and_parity_round_trip_json() {
        for (v, wire) in [
            (DataBits::Five, "\"five\""),
            (DataBits::Six, "\"six\""),
            (DataBits::Seven, "\"seven\""),
            (DataBits::Eight, "\"eight\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), wire);
            let back: DataBits = serde_json::from_str(wire).unwrap();
            assert_eq!(back, v);
        }
        for (v, wire) in [
            (Parity::None, "\"none\""),
            (Parity::Odd, "\"odd\""),
            (Parity::Even, "\"even\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), wire);
            let back: Parity = serde_json::from_str(wire).unwrap();
            assert_eq!(back, v);
        }
    }

    fn temp_serial_debug_dir(label: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "tyutool-serial-debug-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        dir.push(unique);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn archive_backfills_filter_and_reads_match_pages() {
        let dir = temp_serial_debug_dir("filter-page");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();

        let chunk = DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1_700_000_000_000,
            bytes: b"INFO boot\nERR one\nERR two\n".to_vec(),
        };
        let completed = archive.append_chunk(&chunk).unwrap();
        filters.ingest_completed_lines(&completed).unwrap();

        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), archive.total_lines())
            .unwrap();
        filters.backfill_filter(&filter.id, &archive).unwrap();

        let page = filters
            .read_match_page(&filter.id, 0, 10, &archive)
            .unwrap();
        assert_eq!(page.total_matches, 2);
        assert_eq!(
            page.items
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ERR one", "ERR two"]
        );
        let entry = filters.filters.get(&filter.id).unwrap();
        assert_eq!(
            std::fs::metadata(&entry.match_idx_path).unwrap().len(),
            2 * std::mem::size_of::<u64>() as u64
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn filter_tracks_new_lines_beyond_its_history_snapshot() {
        let dir = temp_serial_debug_dir("live-after-create");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();

        let initial = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"ERR old\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&initial).unwrap();

        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), archive.total_lines())
            .unwrap();

        let live = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"ERR new\nOK\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&live).unwrap();
        filters.backfill_filter(&filter.id, &archive).unwrap();

        let stats = filters.stats(&filter.id).unwrap();
        assert_eq!(stats.total_matches, 2);
        let entry = filters.filters.get(&filter.id).unwrap();
        assert_eq!(
            std::fs::metadata(&entry.match_idx_path).unwrap().len(),
            2 * std::mem::size_of::<u64>() as u64
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reset_for_new_session_truncates_filter_match_index_file() {
        let dir = temp_serial_debug_dir("filter-reset");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();

        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"ERR one\nERR two\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&completed).unwrap();
        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), archive.total_lines())
            .unwrap();
        filters.backfill_filter(&filter.id, &archive).unwrap();

        let entry = filters.filters.get(&filter.id).unwrap();
        assert_eq!(
            std::fs::metadata(&entry.match_idx_path).unwrap().len(),
            2 * std::mem::size_of::<u64>() as u64
        );

        let updates = filters.reset_for_new_session();
        let stats = updates
            .into_iter()
            .find(|stats| stats.filter_id == filter.id)
            .unwrap();
        assert_eq!(stats.total_matches, 0);
        assert_eq!(stats.total_lines_snapshot, 0);

        let entry = filters.filters.get(&filter.id).unwrap();
        assert_eq!(std::fs::metadata(&entry.match_idx_path).unwrap().len(), 0);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn backfill_finalize_preserves_live_matches_arriving_during_backfill() {
        let dir = temp_serial_debug_dir("filter-backfill-live-merge");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();

        let historical = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"ERR old\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&historical).unwrap();

        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), archive.total_lines())
            .unwrap();
        filters.start_backfill(&filter.id).unwrap();

        let live = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"ERR live\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&live).unwrap();

        let historical_idx_path = dir.join("historical.idx");
        std::fs::write(&historical_idx_path, 1u64.to_le_bytes()).unwrap();
        let stats = filters
            .finish_backfill_from_file(&filter.id, &historical_idx_path, 1, 1)
            .unwrap();

        assert_eq!(stats.total_matches, 2);
        assert_eq!(stats.total_lines_snapshot, 2);
        let page = filters
            .read_match_page(&filter.id, 0, 10, &archive)
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ERR old", "ERR live"]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_generation_ignores_backfill_completion_after_session_reset() {
        let dir = temp_serial_debug_dir("stale-backfill-generation");
        let generation = SerialDebugGeneration::default();
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();

        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"ERR one\n".to_vec(),
            })
            .unwrap();
        filters.ingest_completed_lines(&completed).unwrap();
        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), archive.total_lines())
            .unwrap();
        let captured_generation = generation.current();
        filters.start_backfill(&filter.id).unwrap();

        generation.advance();
        let _ = filters.reset_for_new_session();
        let historical_idx_path = dir.join("historical.idx");
        std::fs::write(&historical_idx_path, 1u64.to_le_bytes()).unwrap();

        let result = serial_debug_finish_backfill_if_current(
            &generation,
            captured_generation,
            &mut filters,
            &filter.id,
            &historical_idx_path,
            1,
            1,
        )
        .unwrap();
        assert!(result.is_none());

        let stats = filters.stats(&filter.id).unwrap();
        assert_eq!(stats.total_matches, 0);
        assert_eq!(stats.total_lines_snapshot, 0);
        assert_eq!(stats.status, SerialDebugFilterStatus::Complete);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_clear_resets_files_and_line_numbers() {
        let dir = temp_serial_debug_dir("clear");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"before clear\n".to_vec(),
            })
            .unwrap();
        assert_eq!(archive.total_lines(), 1);

        archive.clear().unwrap();
        assert_eq!(archive.total_lines(), 0);

        let lines = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"after clear\n".to_vec(),
            })
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_no, 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_file_ids_are_unique_within_same_millisecond() {
        let first = next_serial_debug_session_id_for_tests(1234, 42);
        let second = next_serial_debug_session_id_for_tests(1234, 42);
        assert_ne!(first, second);
    }

    #[test]
    fn archive_read_page_returns_zero_based_window() {
        let dir = temp_serial_debug_dir("read-page");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"one\ntwo\nthree\n".to_vec(),
            })
            .unwrap();

        let page = archive.read_page(1, 2).unwrap();
        assert_eq!(page.total_lines, 3);
        assert_eq!(page.start, 1);
        assert_eq!(
            page.items
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_read_line_range_returns_one_based_lines_in_order() {
        let dir = temp_serial_debug_dir("read-line-range");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"one\ntwo\nthree\nfour\n".to_vec(),
            })
            .unwrap();

        let items = archive.read_line_range(2, 2).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );

        let tail = archive.read_line_range(4, 3).unwrap();
        assert_eq!(
            tail.iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["four"]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ingest_completed_lines_coalesces_updates_per_filter() {
        let dir = temp_serial_debug_dir("coalesced-filter-updates");
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();
        let filter = filters
            .add_filter("ERR".into(), false, "#f00".into(), 0)
            .unwrap();
        let updates = filters
            .ingest_completed_lines(&[
                SerialDebugLine {
                    line_no: 1,
                    ts_ms: 1,
                    direction: LogDirection::Rx,
                    text: "ERR one".into(),
                    raw_bytes: None,
                },
                SerialDebugLine {
                    line_no: 2,
                    ts_ms: 2,
                    direction: LogDirection::Rx,
                    text: "ERR two".into(),
                    raw_bytes: None,
                },
            ])
            .unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].filter_id, filter.id);
        assert_eq!(updates[0].total_matches, 2);

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Fill the archive until it caps, then return the number of lines written.
    fn fill_until_capped(archive: &mut SerialDebugArchive) -> u64 {
        for i in 0..10_000u64 {
            let before = archive.total_lines();
            archive
                .append_chunk(&DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: i,
                    bytes: format!("line {i} padding padding padding\n").into_bytes(),
                })
                .unwrap();
            if archive.total_lines() == before {
                return archive.total_lines();
            }
        }
        panic!("archive never reached its cap");
    }

    #[test]
    fn capped_archive_stops_numbering_lines_and_keeps_index_consistent() {
        let dir = temp_serial_debug_dir("cap-stop-writing");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive.set_max_bytes(4096);

        let capped_at = fill_until_capped(&mut archive);
        assert!(capped_at > 0, "some lines must land before the cap");

        // The cap must not hand out line numbers for lines that never reached
        // the .idx file — a hole there would make (line_no - 1) * 16 read the
        // wrong entry for every later line, silently.
        let idx_len = std::fs::metadata(&archive.idx_path).unwrap().len();
        assert_eq!(idx_len, capped_at * ARCHIVE_INDEX_ENTRY_BYTES);
        let log_len = std::fs::metadata(&archive.log_path).unwrap().len();
        assert_eq!(log_len, archive.next_offset);
        assert!(log_len <= 4096, "cap must bound the payload: {log_len}");

        // Every numbered line is still readable at its indexed offset.
        let all = archive.read_line_range(1, capped_at).unwrap();
        assert_eq!(all.len() as u64, capped_at);
        assert_eq!(all.last().unwrap().line_no, capped_at);

        // The last archived line announces the cap rather than truncating
        // silently — as a sentinel the frontend translates, never as prose.
        let notice = all.last().unwrap();
        assert_eq!(notice.direction, LogDirection::Sys);
        // Pins the wire format mirrored in
        // src/features/serial-debug/archive-line-text.ts.
        assert_eq!(notice.text, "\u{1}tyutool:archive-capped:1\u{1}");
        assert_eq!(serial_debug_archive_cap_limit_mib(notice), Some(1));

        // Further appends change nothing at all.
        let before = archive.total_lines();
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 9,
                bytes: b"still flowing\n".to_vec(),
            })
            .unwrap();
        assert_eq!(archive.total_lines(), before);
        assert_eq!(
            std::fs::metadata(&archive.idx_path).unwrap().len(),
            idx_len,
            "index must not grow after the cap"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cap_sentinel_is_only_recognised_on_an_exact_sys_line() {
        let sentinel = |direction, text: &str| SerialDebugLine {
            line_no: 1,
            ts_ms: 0,
            direction,
            text: text.to_string(),
            raw_bytes: None,
        };

        assert_eq!(
            serial_debug_archive_cap_limit_mib(&sentinel(
                LogDirection::Sys,
                &serial_debug_archive_cap_sentinel(256)
            )),
            Some(256)
        );

        // A device that echoes the marker byte-for-byte still cannot forge the
        // notice: its output is never a Sys line.
        assert_eq!(
            serial_debug_archive_cap_limit_mib(&sentinel(
                LogDirection::Rx,
                &serial_debug_archive_cap_sentinel(256)
            )),
            None
        );

        // Whole-string match only — an embedding line is not a sentinel.
        for text in [
            &format!("boot: {}", serial_debug_archive_cap_sentinel(256)),
            &format!("{} trailing", serial_debug_archive_cap_sentinel(256)),
            "\u{1}tyutool:archive-capped:\u{1}",
            "\u{1}tyutool:archive-capped:12x\u{1}",
            "tyutool:archive-capped:256",
            "session archive reached its 256 MiB limit",
        ] {
            assert_eq!(
                serial_debug_archive_cap_limit_mib(&sentinel(LogDirection::Sys, text)),
                None,
                "must not match: {text:?}"
            );
        }
    }

    #[test]
    fn chunk_drop_sentinel_is_only_recognised_on_an_exact_sys_line() {
        let line = |direction, text: &str| SerialDebugLine {
            line_no: 1,
            ts_ms: 0,
            direction,
            text: text.to_string(),
            raw_bytes: None,
        };

        assert_eq!(
            serial_debug_chunk_drop_bytes(&line(
                LogDirection::Sys,
                &serial_debug_chunk_drop_sentinel(8192)
            )),
            Some(8192)
        );

        // Device output is never a Sys line, so echoing the marker cannot forge
        // a data-loss notice.
        assert_eq!(
            serial_debug_chunk_drop_bytes(&line(
                LogDirection::Rx,
                &serial_debug_chunk_drop_sentinel(8192)
            )),
            None
        );

        for text in [
            &format!("boot: {}", serial_debug_chunk_drop_sentinel(8192)),
            &format!("{} trailing", serial_debug_chunk_drop_sentinel(8192)),
            "\u{1}tyutool:chunks-dropped:\u{1}",
            "\u{1}tyutool:chunks-dropped:12x\u{1}",
            "tyutool:chunks-dropped:8192",
        ] {
            assert_eq!(
                serial_debug_chunk_drop_bytes(&line(LogDirection::Sys, text)),
                None,
                "must not match: {text:?}"
            );
        }

        // The two sentinel families never answer for each other.
        assert_eq!(
            serial_debug_archive_cap_limit_mib(&line(
                LogDirection::Sys,
                &serial_debug_chunk_drop_sentinel(8192)
            )),
            None
        );
        assert_eq!(
            serial_debug_chunk_drop_bytes(&line(
                LogDirection::Sys,
                &serial_debug_archive_cap_sentinel(256)
            )),
            None
        );
    }

    #[test]
    fn append_gap_closes_the_open_line_so_the_halves_never_merge() {
        let dir = temp_serial_debug_dir("gap-line-boundary");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        // A line that was still being received when the drop happened.
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"before-gap".to_vec(),
            })
            .unwrap();
        assert_eq!(archive.total_lines(), 0, "no newline yet");

        let written = archive.append_gap(Direction::Rx, 2, 4096).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0].direction, LogDirection::Rx);
        assert_eq!(written[0].text, "before-gap");
        assert_eq!(written[1].direction, LogDirection::Sys);
        assert_eq!(serial_debug_chunk_drop_bytes(&written[1]), Some(4096));

        // Bytes that arrive after the gap start a line of their own. Without the
        // cut above they would have been appended to "before-gap", producing one
        // plausible-looking line the device never printed.
        let after = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 3,
                bytes: b"after-gap\n".to_vec(),
            })
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].text, "after-gap");

        let all = archive.read_line_range(1, 10).unwrap();
        let texts = all.iter().map(|l| l.text.as_str()).collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "before-gap",
                serial_debug_chunk_drop_sentinel(4096).as_str(),
                "after-gap",
            ]
        );
        assert!(
            !texts.iter().any(|t| t.contains("before-gapafter-gap")),
            "the gap must be a line boundary, not a splice: {texts:?}"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_gap_only_cuts_its_own_direction_and_needs_no_pending_bytes() {
        let dir = temp_serial_debug_dir("gap-direction");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Tx,
                ts_ms: 1,
                bytes: b"half-sent".to_vec(),
            })
            .unwrap();

        // Nothing buffered on Rx: the gap is just the sentinel.
        let written = archive.append_gap(Direction::Rx, 2, 512).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].direction, LogDirection::Sys);

        // The untouched Tx buffer still completes normally.
        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Tx,
                ts_ms: 3,
                bytes: b"-rest\n".to_vec(),
            })
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].text, "half-sent-rest");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Closing a port must not swallow what the device printed last. A prompt
    /// (`login: `), a progress bar or a bootloader banner carries no trailing
    /// newline, so nothing else in the archive ever cuts it.
    #[test]
    fn finalize_pending_lines_archives_the_unterminated_tail_of_both_directions() {
        let dir = temp_serial_debug_dir("finalize-tail");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Tx,
                ts_ms: 1,
                bytes: b"who".to_vec(),
            })
            .unwrap();
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"login: ".to_vec(),
            })
            .unwrap();
        assert_eq!(archive.total_lines(), 0, "neither side ended in a newline");

        let written = archive.finalize_pending_lines(3).unwrap();

        assert_eq!(written.len(), 2);
        assert_eq!(written[0].direction, LogDirection::Tx);
        assert_eq!(written[0].text, "who");
        assert_eq!(written[1].direction, LogDirection::Rx);
        assert_eq!(written[1].text, "login: ");
        // Readable from the archive file, not just returned: that is what the
        // export, the auto-save backfill and the history window read.
        let all = archive.read_line_range(1, 10).unwrap();
        assert_eq!(
            all.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["who", "login: "]
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finalize_pending_lines_writes_nothing_when_nothing_is_buffered() {
        let dir = temp_serial_debug_dir("finalize-empty");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"done\n".to_vec(),
            })
            .unwrap();

        // Every byte already ended in a line; closing must not add a blank one.
        assert!(archive.finalize_pending_lines(2).unwrap().is_empty());
        assert_eq!(archive.total_lines(), 1);
        // Idempotent: a second close still has nothing to cut.
        assert!(archive.finalize_pending_lines(3).unwrap().is_empty());
        assert_eq!(archive.total_lines(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drop_counter_coalesces_a_burst_into_one_report() {
        let drops = SerialDebugDropCounter::default();

        drops.record(4096, 1_000);
        drops.record(2048, 1_010);
        drops.record(1024, 1_020);

        // Still losing bytes — reporting now would be one notice per chunk.
        assert_eq!(drops.take_report(1_100), None);

        let report = drops
            .take_report(1_020 + SERIAL_DEBUG_DROP_QUIET_MS)
            .unwrap();
        assert_eq!(report.chunks, 3);
        assert_eq!(report.bytes, 4096 + 2048 + 1024);

        // One burst, one notice: the counters are empty afterwards.
        assert_eq!(drops.take_report(9_999_999), None);
        assert_eq!(drops.take_pending(), None);
    }

    #[test]
    fn drop_counter_reports_a_burst_that_never_goes_quiet() {
        let drops = SerialDebugDropCounter::default();

        // A device that keeps flooding never leaves a quiet window, and the user
        // is exactly the person who can act on this — so the burst ceiling has to
        // force a report out.
        let mut now = 1_000;
        while now < 1_000 + SERIAL_DEBUG_DROP_BURST_MS {
            drops.record(4096, now);
            assert_eq!(drops.take_report(now), None);
            now += 10;
        }
        drops.record(4096, now);
        let report = drops.take_report(now).unwrap();
        assert!(report.chunks > 1, "the burst must be coalesced");
        assert_eq!(report.bytes, report.chunks * 4096);
    }

    #[test]
    fn drop_counter_take_pending_drains_an_unfinished_burst() {
        let drops = SerialDebugDropCounter::default();
        assert_eq!(drops.take_pending(), None);

        drops.record(64, 1_000);
        // Teardown / session clear must not need the timing gate.
        assert_eq!(
            drops.take_pending(),
            Some(SerialDebugDropReport {
                chunks: 1,
                bytes: 64
            })
        );
        assert_eq!(drops.take_pending(), None);
    }

    #[test]
    fn capped_archive_returns_no_lines_and_freezes_filter_matches() {
        let dir = temp_serial_debug_dir("cap-filter-freeze");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let mut filters = SerialDebugFilterIndex::create(&dir).unwrap();
        let filter = filters
            .add_filter("line".into(), false, "#f00".into(), 0)
            .unwrap();
        archive.set_max_bytes(4096);

        loop {
            let completed = archive
                .append_chunk(&DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: 1,
                    bytes: b"line padding padding padding padding\n".to_vec(),
                })
                .unwrap();
            filters.ingest_completed_lines(&completed).unwrap();
            if completed.is_empty() {
                break;
            }
        }
        let frozen = filters.stats(&filter.id).unwrap().total_matches;

        for _ in 0..5 {
            let completed = archive
                .append_chunk(&DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: 2,
                    bytes: b"line after the cap\n".to_vec(),
                })
                .unwrap();
            assert!(completed.is_empty(), "capped archive yields no lines");
            filters.ingest_completed_lines(&completed).unwrap();
        }
        assert_eq!(filters.stats(&filter.id).unwrap().total_matches, frozen);

        // Sys lines are dropped too, and report it.
        assert!(archive.append_sys_line(3, "note".into()).unwrap().is_none());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clear_resets_the_capped_state() {
        let dir = temp_serial_debug_dir("cap-clear");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive.set_max_bytes(4096);
        fill_until_capped(&mut archive);

        archive.clear().unwrap();
        assert_eq!(archive.total_lines(), 0);

        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"after clear\n".to_vec(),
            })
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].line_no, 1);
        assert_eq!(completed[0].text, "after clear");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn raising_the_limit_resumes_archiving_a_capped_session() {
        let dir = temp_serial_debug_dir("cap-raise");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        archive.set_max_bytes(4096);
        let capped_at = fill_until_capped(&mut archive);

        archive.set_max_bytes(64 * 1024);
        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"after raise\n".to_vec(),
            })
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].line_no, capped_at + 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_lines_do_not_carry_raw_bytes_into_the_session_file() {
        let dir = temp_serial_debug_dir("no-raw-bytes");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: b"hello\n".to_vec(),
            })
            .unwrap();
        assert_eq!(completed[0].text, "hello");
        assert!(completed[0].raw_bytes.is_none());

        let raw = std::fs::read_to_string(&archive.log_path).unwrap();
        assert!(
            !raw.contains("rawBytes"),
            "archive JSON must stay lean: {raw}"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prune_removes_session_pairs_but_never_live_filter_indexes() {
        let dir = temp_serial_debug_dir("prune-keeps-filter-idx");
        // A live filter's match index lives in the same directory and shares the
        // `.idx` extension — pruning by extension would delete it mid-session.
        let filter_idx = dir.join("serial-debug-filter-filter-1.idx");
        let filter_live_idx = dir.join("serial-debug-filter-filter-1.live.idx");
        std::fs::write(&filter_idx, b"keep me").unwrap();
        std::fs::write(&filter_live_idx, b"keep me too").unwrap();

        let mut created = Vec::new();
        for i in 0..(MAX_ARCHIVE_SESSIONS + 3) {
            let ndjson = dir.join(format!(
                "{SERIAL_DEBUG_SESSION_FILE_PREFIX}{i:04}-1-0.ndjson"
            ));
            let idx = dir.join(format!("{SERIAL_DEBUG_SESSION_FILE_PREFIX}{i:04}-1-0.idx"));
            std::fs::write(&ndjson, b"{}\n").unwrap();
            std::fs::write(&idx, [0u8; 16]).unwrap();
            // Distinct mtimes: prune orders by mtime, oldest first.
            std::thread::sleep(std::time::Duration::from_millis(5));
            created.push((ndjson, idx));
        }

        prune_serial_debug_archives(&dir);

        assert!(filter_idx.exists(), "live filter match index must survive");
        assert!(
            filter_live_idx.exists(),
            "pending live filter match index must survive"
        );

        let surviving = created.iter().filter(|(ndjson, _)| ndjson.exists()).count();
        assert_eq!(surviving, MAX_ARCHIVE_SESSIONS);
        for (ndjson, idx) in &created {
            assert_eq!(
                ndjson.exists(),
                idx.exists(),
                "both halves of a pair must go together: {ndjson:?}"
            );
        }
        // The newest pairs are the ones kept.
        assert!(created.last().unwrap().0.exists());
        assert!(!created.first().unwrap().0.exists());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_chunk_forces_long_pending_data_into_bounded_lines() {
        let dir = temp_serial_debug_dir("force-split-long-line");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        let long = vec![b'a'; MAX_PENDING_SERIAL_DEBUG_LINE_BYTES + 8];

        let completed = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 1,
                bytes: long,
            })
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].text.len(), MAX_PENDING_SERIAL_DEBUG_LINE_BYTES);
        assert_eq!(archive.total_lines(), 1);

        let completed_tail = archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 2,
                bytes: b"\n".to_vec(),
            })
            .unwrap();
        assert_eq!(completed_tail.len(), 1);
        assert_eq!(completed_tail[0].text.len(), 8);

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The pre-batching reader: seek+read the index entry, then seek+read the
    /// payload, once per line. Kept here as the reference that the batched
    /// `read_archive_lines` has to reproduce exactly.
    fn read_lines_one_at_a_time(
        log_path: &std::path::Path,
        idx_path: &std::path::Path,
        line_nos: &[u64],
        max_line_no: Option<u64>,
    ) -> Vec<SerialDebugLine> {
        let mut idx_file = File::open(idx_path).unwrap();
        let mut log_file = File::open(log_path).unwrap();
        let mut items = Vec::new();
        for &line_no in line_nos {
            if line_no == 0 || max_line_no.is_some_and(|max| line_no > max) {
                continue;
            }
            idx_file
                .seek(SeekFrom::Start((line_no - 1) * ARCHIVE_INDEX_ENTRY_BYTES))
                .unwrap();
            let mut offset_buf = [0u8; 8];
            let mut len_buf = [0u8; 8];
            idx_file.read_exact(&mut offset_buf).unwrap();
            idx_file.read_exact(&mut len_buf).unwrap();
            let offset = u64::from_le_bytes(offset_buf);
            let len = u64::from_le_bytes(len_buf);
            log_file.seek(SeekFrom::Start(offset)).unwrap();
            let mut buf = vec![0u8; len as usize];
            log_file.read_exact(&mut buf).unwrap();
            items.push(serde_json::from_slice::<SerialDebugLine>(&buf).unwrap());
        }
        items
    }

    /// `read_page(start, limit)` must return exactly what the one-line-at-a-time
    /// reader returns for the same window, and must keep clamping `start`
    /// silently — the frontend reads `items[0].lineNo`, but `page.start` is part
    /// of the contract all the same.
    fn assert_read_page_matches_reference(archive: &SerialDebugArchive, start: u64, limit: u64) {
        let total = archive.total_lines();
        let clamped_start = start.min(total);
        let end = clamped_start.saturating_add(limit).min(total);
        let expected = read_lines_one_at_a_time(
            &archive.log_path,
            &archive.idx_path,
            &(clamped_start + 1..=end).collect::<Vec<_>>(),
            Some(total),
        );

        let page = archive.read_page(start, limit).unwrap();
        assert_eq!(page.total_lines, total, "start={start} limit={limit}");
        assert_eq!(page.start, clamped_start, "start={start} limit={limit}");
        assert_eq!(page.items, expected, "start={start} limit={limit}");
    }

    fn append_numbered_lines(archive: &mut SerialDebugArchive, count: u64) {
        let mut bytes = Vec::new();
        for i in 1..=count {
            bytes.extend_from_slice(format!("line {i} payload\n").as_bytes());
        }
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 7,
                bytes,
            })
            .unwrap();
    }

    #[test]
    fn archive_read_page_matches_line_by_line_reads() {
        let dir = temp_serial_debug_dir("read-page-equivalence");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        // Empty archive: every window is empty and `start` clamps to 0.
        assert_read_page_matches_reference(&archive, 0, 400);
        assert_read_page_matches_reference(&archive, 9, 400);

        // Single line.
        append_numbered_lines(&mut archive, 1);
        assert_read_page_matches_reference(&archive, 0, 400);
        assert_read_page_matches_reference(&archive, 1, 400);
        assert_read_page_matches_reference(&archive, 5, 400);

        // 907 lines over a 400-line page size: two full pages, a short last
        // page, and both page boundaries.
        append_numbered_lines(&mut archive, 906);
        assert_eq!(archive.total_lines(), 907);
        for start in [0, 1, 399, 400, 401, 799, 800, 906, 907] {
            assert_read_page_matches_reference(&archive, start, 400);
        }
        // `start` past the end is clamped silently, not rejected.
        for start in [908, 5_000, u64::MAX] {
            assert_read_page_matches_reference(&archive, start, 400);
        }
        assert!(archive.read_page(908, 400).unwrap().items.is_empty());
        assert_eq!(archive.read_page(908, 400).unwrap().start, 907);
        // Degenerate limits.
        assert_read_page_matches_reference(&archive, 10, 0);
        assert_read_page_matches_reference(&archive, 10, 1);
        assert_read_page_matches_reference(&archive, 0, u64::MAX);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_read_page_matches_line_by_line_reads_with_gap_and_cap_lines() {
        let dir = temp_serial_debug_dir("read-page-equivalence-sentinels");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();

        // A gap closes off the buffered partial line and appends a sentinel, so
        // the archive holds an ordinary line, a partial line and a `Sys` line
        // back to back — the contiguous-span assumption has to hold across all
        // three.
        append_numbered_lines(&mut archive, 3);
        archive
            .append_chunk(&DebugChunk {
                direction: Direction::Rx,
                ts_ms: 8,
                bytes: b"truncated".to_vec(),
            })
            .unwrap();
        archive.append_gap(Direction::Rx, 9, 128).unwrap();
        append_numbered_lines(&mut archive, 3);
        archive.append_sys_line(10, "note".into()).unwrap();
        assert_eq!(archive.total_lines(), 9);
        for start in 0..=10 {
            assert_read_page_matches_reference(&archive, start, 4);
        }

        // Then fill to the cap: the last archived line is the cap sentinel and
        // nothing is written after it, so the index stays hole-free.
        archive.set_max_bytes(8192);
        let capped_at = fill_until_capped(&mut archive);
        assert!(capped_at > 9);
        assert!(
            serial_debug_archive_cap_limit_mib(
                archive
                    .read_page(capped_at - 1, 1)
                    .unwrap()
                    .items
                    .first()
                    .unwrap()
            )
            .is_some(),
            "the last line must be the cap sentinel"
        );
        for start in [0, capped_at / 2, capped_at - 1, capped_at, capped_at + 5] {
            assert_read_page_matches_reference(&archive, start, 400);
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_read_page_matches_line_by_line_reads_across_read_batches() {
        let dir = temp_serial_debug_dir("read-page-equivalence-batches");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        // 1200 near-maximal lines: more than ARCHIVE_READ_INDEX_BATCH_LINES
        // (1024) and over 4 MiB of payload, so a single-window read has to split
        // both its index reads and its payload spans.
        let filler = "x".repeat(4000);
        for batch in 0..12 {
            let mut bytes = Vec::new();
            for i in 0..100 {
                bytes.extend_from_slice(format!("{}-{i} {filler}\n", batch).as_bytes());
            }
            archive
                .append_chunk(&DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: batch,
                    bytes,
                })
                .unwrap();
        }
        assert_eq!(archive.total_lines(), 1200);
        assert_read_page_matches_reference(&archive, 0, 1200);
        assert_read_page_matches_reference(&archive, 1000, 400);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_read_lines_matches_line_by_line_reads_for_arbitrary_line_numbers() {
        let dir = temp_serial_debug_dir("read-lines-equivalence");
        let mut archive = SerialDebugArchive::create(&dir).unwrap();
        append_numbered_lines(&mut archive, 20);

        // Sparse (a filter match list), descending, duplicated, line 0 and past
        // the end all have to behave as before: skip what is out of range, keep
        // the caller's order everywhere else.
        for line_nos in [
            vec![],
            vec![0],
            vec![0, 1, 0, 2],
            vec![3, 7, 8, 9, 15],
            vec![20, 19, 18],
            vec![5, 5, 5, 6],
            vec![18, 19, 20, 21, 22],
            (1..=20).collect::<Vec<_>>(),
        ] {
            let expected = read_lines_one_at_a_time(
                &archive.log_path,
                &archive.idx_path,
                &line_nos,
                Some(archive.total_lines()),
            );
            assert_eq!(
                archive.read_lines(&line_nos).unwrap(),
                expected,
                "{line_nos:?}"
            );

            // The snapshot reader has no total to clamp against; within the
            // archive it must agree line for line.
            if line_nos.iter().all(|&n| n <= 20) {
                let reader_expected =
                    read_lines_one_at_a_time(&archive.log_path, &archive.idx_path, &line_nos, None);
                assert_eq!(
                    archive.snapshot_reader().read_lines(&line_nos).unwrap(),
                    reader_expected,
                    "{line_nos:?}"
                );
            }
        }

        std::fs::remove_dir_all(dir).unwrap();
    }
}
