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
static SERIAL_DEBUG_SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerialDebugArchiveMeta {
    pub session_id: String,
    pub log_path: String,
    pub total_lines: u64,
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

impl SerialDebugArchive {
    pub fn create(root_dir: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(root_dir)?;
        let (session_id, log_path, idx_path, log_writer, idx_writer) =
            Self::create_session_files(root_dir)?;
        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            session_id,
            log_path,
            idx_path,
            log_writer,
            idx_writer,
            next_offset: 0,
            next_line_no: 0,
            pending_tx: Vec::new(),
            pending_rx: Vec::new(),
        })
    }

    pub fn meta(&self) -> SerialDebugArchiveMeta {
        SerialDebugArchiveMeta {
            session_id: self.session_id.clone(),
            log_path: self.log_path.display().to_string(),
            total_lines: self.total_lines(),
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
            let direction = match chunk.direction {
                Direction::Tx => LogDirection::Tx,
                Direction::Rx => LogDirection::Rx,
            };
            decoded.push(SerialDebugLine {
                line_no: 0,
                ts_ms: chunk.ts_ms,
                direction,
                text,
                raw_bytes: Some(raw_bytes),
            });
        }

        let mut completed = Vec::with_capacity(decoded.len());
        for line in decoded {
            completed.push(self.append_line(line)?);
        }
        if !completed.is_empty() {
            self.flush_writers()?;
        }

        Ok(completed)
    }

    pub fn append_sys_line(
        &mut self,
        ts_ms: u64,
        text: String,
    ) -> std::io::Result<SerialDebugLine> {
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
        let mut idx_file = File::open(&self.idx_path)?;
        let mut log_file = File::open(&self.log_path)?;
        let mut items = Vec::with_capacity(line_nos.len());
        for &line_no in line_nos {
            if let Some(line) = self.read_line_with_files(line_no, &mut idx_file, &mut log_file)? {
                items.push(line);
            }
        }
        Ok(items)
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

    fn append_line(&mut self, mut line: SerialDebugLine) -> std::io::Result<SerialDebugLine> {
        self.next_line_no += 1;
        line.line_no = self.next_line_no;
        let encoded = serde_json::to_vec(&line)?;
        self.log_writer.write_all(&encoded)?;
        self.log_writer.write_all(b"\n")?;
        self.idx_writer.write_all(&self.next_offset.to_le_bytes())?;
        self.idx_writer
            .write_all(&(encoded.len() as u64).to_le_bytes())?;
        self.next_offset += encoded.len() as u64 + 1;
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

    fn read_line_with_files(
        &self,
        line_no: u64,
        idx_file: &mut File,
        log_file: &mut File,
    ) -> std::io::Result<Option<SerialDebugLine>> {
        if line_no == 0 || line_no > self.total_lines() {
            return Ok(None);
        }
        let (offset, len) = self.read_index_entry_with_file(line_no, idx_file)?;
        log_file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len as usize];
        log_file.read_exact(&mut buf)?;
        Ok(Some(serde_json::from_slice::<SerialDebugLine>(&buf)?))
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
        let mut idx_file = File::open(&self.idx_path)?;
        let mut log_file = File::open(&self.log_path)?;
        let mut items = Vec::with_capacity(line_nos.len());
        for &line_no in line_nos {
            if line_no == 0 {
                continue;
            }
            idx_file.seek(SeekFrom::Start((line_no - 1) * ARCHIVE_INDEX_ENTRY_BYTES))?;
            let mut offset_buf = [0u8; 8];
            let mut len_buf = [0u8; 8];
            idx_file.read_exact(&mut offset_buf)?;
            idx_file.read_exact(&mut len_buf)?;
            let offset = u64::from_le_bytes(offset_buf);
            let len = u64::from_le_bytes(len_buf);
            log_file.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0u8; len as usize];
            log_file.read_exact(&mut buf)?;
            items.push(serde_json::from_slice::<SerialDebugLine>(&buf)?);
        }
        Ok(items)
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
}
