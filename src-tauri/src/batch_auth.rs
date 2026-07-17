// src-tauri/src/batch_auth.rs
//! Excel-based authorization row allocator for batch auth.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use calamine::{open_workbook_auto, Reader};
use rust_xlsxwriter::{Format, Workbook as XlsxWorkbook};

#[derive(Debug, Clone)]
struct HeaderInfo {
    uuid_col: usize,
    authkey_col: usize,
    status_col: Option<usize>,
    mac_col: Option<usize>,
    timestamp_col: Option<usize>,
    step_col: Option<usize>,
    error_col: Option<usize>,
    total_cols: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    Available,
    /// MAC 已读取并绑定到本行，凭据已分配，但 auth 命令尚未发出。
    MacRead,
    /// auth 写命令已发出；OTP 可能已烧。此状态起永远不归还 Available。
    AuthWritten,
    /// auth-read 验证通过。
    AuthVerified,
    /// 历史遗留状态：旧版本的 auth-otp-lock 成功会写入 OTPLOCKED。
    /// 当前固件已移除 OTP 锁定命令，不再产生此状态，仅为读取旧 Excel 保留。
    OtpLocked,
    /// 完整流程结束（对应 AlreadyDone / 正常成功）。
    Done,
}

#[derive(Debug, Clone)]
struct RowData {
    uuid: String,
    authkey: String,
    status: RowStatus,
    mac: Option<String>,
    timestamp: Option<String>,
    step: Option<String>,
    last_error: Option<String>,
    extra_cells: Vec<(usize, String)>,
    /// UUID/AuthKey pass the firmware length rules. Computed on load.
    valid: bool,
}

#[derive(Debug)]
pub struct ExcelRow {
    pub row_idx: usize,
    pub uuid: String,
    pub authkey: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelStats {
    pub total: usize,
    pub used: usize,
    pub in_progress: usize,
    pub remaining: usize,
    /// Rows whose UUID/AuthKey fail the firmware length rules (UUID 16/20,
    /// AuthKey 32). These are never allocated and never counted as `remaining`.
    pub invalid: usize,
}

/// Mirror of the firmware's credential-length rules (`tuya_authorize.c`).
/// A row failing this is a data-entry error that would be rejected by the
/// device, so it must not be handed out or burned.
fn credentials_valid(uuid: &str, authkey: &str) -> bool {
    let ul = uuid.chars().count();
    let kl = authkey.chars().count();
    (ul == 16 || ul == 20) && kl == 32
}

struct AllocatorState {
    path: PathBuf,
    header: HeaderInfo,
    header_raw: Vec<String>,
    rows: Vec<RowData>,
    backed_up: bool,
    /// Persistent write handle held for the duration of a batch run. On
    /// Windows it denies other writers (Excel/WPS can only open read-only)
    /// while readers keep working. `None` for plain (validation) loads.
    lock_handle: Option<std::fs::File>,
}

#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg(windows)]
const FILE_SHARE_DELETE: u32 = 0x4;

fn open_lock_handle_io(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Share read + delete, deny write: validation reads stay functional,
        // Excel/WPS cannot grab a write handle, and our own tmp+rename save
        // (which needs delete sharing on the target) is not blocked by us.
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE);
    }
    opts.open(path)
}

/// Copy via read+write. `std::fs::copy` (CopyFileExW on Windows) opens the
/// source denying write sharing, which conflicts with our own held write
/// handle; a plain read open shares everything and works under the lock.
fn copy_file_shared(src: &Path, dst: &Path) -> std::io::Result<()> {
    let bytes = std::fs::read(src)?;
    std::fs::write(dst, bytes)
}

/// Retry a fallible fs op a few times: transient read handles (antivirus,
/// search indexers) can briefly block rename/open on Windows.
fn retry_briefly<T, E>(mut op: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    let mut last = op();
    for _ in 0..4 {
        if last.is_ok() {
            return last;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        last = op();
    }
    last
}

pub struct ExcelRowAllocator {
    state: Mutex<AllocatorState>,
}

impl ExcelRowAllocator {
    pub fn path_matches(&self, path: &Path) -> bool {
        self.state.lock().map(|s| s.path == path).unwrap_or(false)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        Self::load_inner(path, None)
    }

    /// Like [`Self::load`], but first acquires a persistent write handle that
    /// is held until [`Self::release_lock`] (or drop). If another program
    /// (Excel/WPS) has the file open for writing, fails fast with
    /// `"excel.locked"`. Used only by batch runs; validation keeps the plain
    /// read-only `load`.
    pub fn load_locked(path: &Path) -> Result<Self, String> {
        // Lock before parsing so the content we read cannot change afterwards
        // (calamine's read-only open coexists with our handle).
        let lock = open_lock_handle_io(path).map_err(|e| {
            log::warn!("[batch-auth] excel lock open failed: {e}");
            "excel.locked".to_string()
        })?;
        log::info!("[batch-auth] excel lock acquired: {}", path.display());
        Self::load_inner(path, Some(lock))
    }

    /// Release the write lock (no-op for plain loads or if already released).
    pub fn release_lock(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.lock_handle.take().is_some() {
                log::info!("[batch-auth] excel lock released: {}", state.path.display());
            }
        }
    }

    fn load_inner(path: &Path, lock_handle: Option<std::fs::File>) -> Result<Self, String> {
        let mut wb =
            open_workbook_auto(path).map_err(|e| format!("Cannot open Excel file: {e}"))?;

        let sheet_name = wb
            .sheet_names()
            .first()
            .cloned()
            .ok_or("Excel file has no sheets")?;

        let range = wb
            .worksheet_range(&sheet_name)
            .map_err(|e| format!("Cannot read sheet '{sheet_name}': {e}"))?;

        let mut rows_iter = range.rows();

        let header_row = rows_iter
            .next()
            .ok_or("Excel file is empty (no header row)")?;

        let header_strings: Vec<String> = header_row
            .iter()
            .map(|c| c.to_string().trim().to_string())
            .collect();

        let find_col = |names: &[&str]| -> Option<usize> {
            header_strings.iter().position(|h| {
                let lower = h.to_lowercase();
                names.iter().any(|&n| lower == n)
            })
        };

        let uuid_col = find_col(&["uuid"]).ok_or("Missing required column: UUID")?;
        let authkey_col =
            find_col(&["authkey", "key"]).ok_or("Missing required column: AUTHKEY (or key)")?;
        let status_col = find_col(&["status"]);
        let mac_col = find_col(&["mac"]);
        let timestamp_col = find_col(&["timestamp"]);
        let step_col = find_col(&["step"]);
        let error_col = find_col(&["error", "last_error"]);
        let total_cols = header_strings.len();

        let header = HeaderInfo {
            uuid_col,
            authkey_col,
            status_col,
            mac_col,
            timestamp_col,
            step_col,
            error_col,
            total_cols,
        };

        let mut rows: Vec<RowData> = Vec::new();
        for data_row in rows_iter {
            let get = |idx: usize| -> String {
                data_row
                    .get(idx)
                    .map(|c| c.to_string().trim().to_string())
                    .unwrap_or_default()
            };

            let uuid = get(uuid_col);
            if uuid.is_empty() {
                continue;
            }

            let authkey = get(authkey_col);
            let status_str = status_col.map(|i| get(i)).unwrap_or_default();
            let step_str = step_col.map(|i| get(i)).unwrap_or_default();
            let error_str = error_col.map(|i| get(i)).filter(|s| !s.is_empty());
            let mac = mac_col.map(|i| get(i)).filter(|s| !s.is_empty());
            let timestamp = timestamp_col.map(|i| get(i)).filter(|s| !s.is_empty());

            let status = match status_str.to_uppercase().as_str() {
                "MACREAD" => RowStatus::MacRead,
                "AUTHWRITTEN" => RowStatus::AuthWritten,
                "AUTHVERIFIED" => RowStatus::AuthVerified,
                "OTPLOCKED" => RowStatus::OtpLocked,
                "DONE" => RowStatus::Done,
                _ => RowStatus::Available,
            };

            let known: HashSet<usize> = [
                Some(uuid_col),
                Some(authkey_col),
                status_col,
                mac_col,
                timestamp_col,
                step_col,
                error_col,
            ]
            .into_iter()
            .flatten()
            .collect();

            let extra_cells = (0..data_row.len())
                .filter(|i| !known.contains(i))
                .map(|i| (i, get(i)))
                .collect();

            let valid = credentials_valid(&uuid, &authkey);
            if !valid {
                log::warn!(
                    "[batch-auth] invalid credential length in Excel: uuid_len={} authkey_len={}",
                    uuid.chars().count(),
                    authkey.chars().count()
                );
            }

            rows.push(RowData {
                uuid,
                authkey,
                status,
                mac,
                timestamp,
                step: if step_str.is_empty() {
                    None
                } else {
                    Some(step_str)
                },
                last_error: error_str,
                extra_cells,
                valid,
            });
        }

        // Warn on duplicate non-empty MAC bindings: find_by_mac returns the
        // first match, so a duplicated MAC (manual edit, copy/paste) would route
        // a device to the wrong row. We don't block loading — just surface it.
        let mut seen_macs = HashSet::new();
        for row in &rows {
            if let Some(mac) = row.mac.as_deref().filter(|m| !m.is_empty()) {
                if !seen_macs.insert(mac.to_string()) {
                    log::warn!("[batch-auth] duplicate MAC in Excel: {mac}");
                }
            }
        }

        Ok(Self {
            state: Mutex::new(AllocatorState {
                path: path.to_owned(),
                header,
                header_raw: header_strings,
                rows,
                backed_up: false,
                lock_handle,
            }),
        })
    }

    pub fn stats(&self) -> ExcelStats {
        let state = self.state.lock().unwrap();
        let total = state.rows.len();
        let used = state
            .rows
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    RowStatus::AuthVerified | RowStatus::OtpLocked | RowStatus::Done
                )
            })
            .count();
        let in_progress = state
            .rows
            .iter()
            .filter(|r| matches!(r.status, RowStatus::MacRead | RowStatus::AuthWritten))
            .count();
        let remaining = state
            .rows
            .iter()
            .filter(|r| r.status == RowStatus::Available && r.valid)
            .count();
        let invalid = state.rows.iter().filter(|r| !r.valid).count();
        ExcelStats {
            total,
            used,
            in_progress,
            remaining,
            invalid,
        }
    }

    pub fn allocate_row(&self) -> Result<ExcelRow, String> {
        let mut state = self.state.lock().unwrap();
        for (idx, row) in state.rows.iter_mut().enumerate() {
            if row.status == RowStatus::Available && row.valid {
                row.status = RowStatus::MacRead;
                return Ok(ExcelRow {
                    row_idx: idx,
                    uuid: row.uuid.clone(),
                    authkey: row.authkey.clone(),
                });
            }
        }
        Err("Authorization codes exhausted — no available rows in Excel".into())
    }

    /// 按 MAC 查找已绑定的行。返回 `(row_idx, uuid, authkey)`，未找到返回 `None`。
    pub fn find_by_mac(&self, mac: &str) -> Option<(usize, String, String)> {
        let state = self.state.lock().unwrap();
        state.rows.iter().enumerate().find_map(|(i, r)| {
            if r.mac.as_deref() == Some(mac) {
                Some((i, r.uuid.clone(), r.authkey.clone()))
            } else {
                None
            }
        })
    }

    /// Skip 场景:设备已自带授权码。若该 UUID 命中本表某「仍 Available」的行,
    /// 绑定 MAC 并标记 Done,避免同一码再被分配给其他设备。
    /// 已被占用(MacRead 及之后任意状态)的行不受影响——只认领尚未发出的码。
    pub fn confirm_existing_uuid(&self, uuid: &str, mac: &str) -> Result<(), String> {
        let row_idx = {
            let state = self.state.lock().unwrap();
            state
                .rows
                .iter()
                .position(|r| r.uuid == uuid && r.status == RowStatus::Available)
        };
        match row_idx {
            None => Ok(()), // 码不在本表或已占用,无需处理
            Some(idx) => self.update_row_state(idx, mac, RowStatus::Done, None, None),
        }
    }

    /// 更新行状态并写入磁盘。每个关键步骤完成后调用一次。
    /// `mac`：本次读到的设备 MAC，首次调用时写入行；`error`：失败原因（可选）。
    pub fn update_row_state(
        &self,
        row_idx: usize,
        mac: &str,
        status: RowStatus,
        step_name: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), String> {
        log::info!(
            "[batch-auth] excel-update  row={row_idx} mac={mac} status={status:?} step={} error={}",
            step_name.unwrap_or("-"),
            error.unwrap_or("-"),
        );
        let mut state = self.state.lock().unwrap();

        if !state.backed_up {
            let bak = state.path.with_extension("xlsx.bak");
            if !bak.exists() {
                copy_file_shared(&state.path, &bak).ok();
            }
            state.backed_up = true;
        }

        if let Some(row) = state.rows.get_mut(row_idx) {
            row.status = status;
            if row.mac.is_none() || row.mac.as_deref() == Some("") {
                row.mac = Some(mac.to_string());
            }
            row.timestamp = Some(utc_now_iso8601());
            if let Some(s) = step_name {
                row.step = Some(s.to_string());
            }
            row.last_error = error.map(|e| e.to_string());
        }
        save_workbook(&mut state)
    }
}

fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = unix_to_utc(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn unix_to_utc(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let s = secs % 60;
    let mins = secs / 60;
    let mi = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let days = (hours / 24) as i32;

    let mut y = 1970i32;
    let mut remaining = days;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if remaining < dy {
            break;
        }
        remaining -= dy;
        y += 1;
    }
    let month_days = [
        31i32,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1u32;
    for &dm in &month_days {
        if remaining < dm {
            break;
        }
        remaining -= dm;
        mo += 1;
    }
    (y, mo, remaining as u32 + 1, h as u32, mi as u32, s as u32)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn save_workbook(state: &mut AllocatorState) -> Result<(), String> {
    let h = &state.header;

    let mut next_col = h.total_cols;
    let status_col = h.status_col.unwrap_or_else(|| {
        let c = next_col;
        next_col += 1;
        c
    });
    let mac_col = h.mac_col.unwrap_or_else(|| {
        let c = next_col;
        next_col += 1;
        c
    });
    let ts_col = h.timestamp_col.unwrap_or_else(|| {
        let c = next_col;
        next_col += 1;
        c
    });
    let step_col = h.step_col.unwrap_or_else(|| {
        let c = next_col;
        next_col += 1;
        c
    });
    let error_col = h.error_col.unwrap_or_else(|| {
        let c = next_col;
        next_col += 1;
        c
    });

    let mut wb = XlsxWorkbook::new();
    let ws = wb.add_worksheet();
    let bold = Format::new().set_bold();

    // Header row
    for (col, text) in state.header_raw.iter().enumerate() {
        ws.write_with_format(0, col as u16, text.as_str(), &bold)
            .map_err(|e| e.to_string())?;
    }
    if h.status_col.is_none() {
        ws.write_with_format(0, status_col as u16, "STATUS", &bold)
            .map_err(|e| e.to_string())?;
    }
    if h.mac_col.is_none() {
        ws.write_with_format(0, mac_col as u16, "MAC", &bold)
            .map_err(|e| e.to_string())?;
    }
    if h.timestamp_col.is_none() {
        ws.write_with_format(0, ts_col as u16, "TIMESTAMP", &bold)
            .map_err(|e| e.to_string())?;
    }
    if h.step_col.is_none() {
        ws.write_with_format(0, step_col as u16, "STEP", &bold)
            .map_err(|e| e.to_string())?;
    }
    if h.error_col.is_none() {
        ws.write_with_format(0, error_col as u16, "ERROR", &bold)
            .map_err(|e| e.to_string())?;
    }

    // Data rows
    for (i, row) in state.rows.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write(r, h.uuid_col as u16, row.uuid.as_str())
            .map_err(|e| e.to_string())?;
        ws.write(r, h.authkey_col as u16, row.authkey.as_str())
            .map_err(|e| e.to_string())?;
        let status_str = match row.status {
            RowStatus::Available => "",
            RowStatus::MacRead => "MACREAD",
            RowStatus::AuthWritten => "AUTHWRITTEN",
            RowStatus::AuthVerified => "AUTHVERIFIED",
            RowStatus::OtpLocked => "OTPLOCKED",
            RowStatus::Done => "DONE",
        };
        ws.write(r, status_col as u16, status_str)
            .map_err(|e| e.to_string())?;
        if let Some(ref mac) = row.mac {
            ws.write(r, mac_col as u16, mac.as_str())
                .map_err(|e| e.to_string())?;
        }
        if let Some(ref ts) = row.timestamp {
            ws.write(r, ts_col as u16, ts.as_str())
                .map_err(|e| e.to_string())?;
        }
        if let Some(ref step) = row.step {
            ws.write(r, step_col as u16, step.as_str())
                .map_err(|e| e.to_string())?;
        }
        if let Some(ref err) = row.last_error {
            ws.write(r, error_col as u16, err.as_str())
                .map_err(|e| e.to_string())?;
        }
        for &(col, ref val) in &row.extra_cells {
            ws.write(r, col as u16, val.as_str())
                .map_err(|e| e.to_string())?;
        }
    }

    // Atomic write: build into a temp file, then rename over the target so a
    // crash/power-loss mid-write can never truncate the ledger — the ledger is
    // the ONLY record of which OTP codes were burned to which devices.
    let tmp = state.path.with_extension("xlsx.tmp");
    wb.save(&tmp)
        .map_err(|e| format!("Failed to write temp Excel: {e}"))?;

    // Roll the current (last known-good) file into a snapshot before we
    // overwrite it. Two backups exist: `.xlsx.bak` = pristine original (made
    // once, in update_row_state), `.xlsx.prev.bak` = last successful save.
    if state.path.exists() {
        let prev = state.path.with_extension("xlsx.prev.bak");
        if let Err(e) = copy_file_shared(&state.path, &prev) {
            log::warn!("[batch-auth] prev-snapshot backup failed: {e}");
        }
    }

    // The lock handle must not be held across the rename: even when the
    // replace succeeds, the old handle would keep protecting the unlinked
    // pre-save file instead of the new one. Drop, rename, reacquire.
    let had_lock = state.lock_handle.take().is_some();

    let renamed = retry_briefly(|| std::fs::rename(&tmp, &state.path));

    // Reacquire regardless of the rename outcome so the remaining rows stay
    // protected for the rest of the batch. A miss self-heals on the next
    // save (take() on None is a no-op, reopen is retried here again).
    if had_lock {
        match retry_briefly(|| open_lock_handle_io(&state.path)) {
            Ok(f) => state.lock_handle = Some(f),
            Err(e) => {
                log::error!("[batch-auth] excel re-lock failed after save: {e}");
                if renamed.is_ok() {
                    return Err(format!(
                        "Excel saved, but re-locking it failed (close the program using the file): {e}"
                    ));
                }
            }
        }
    }

    renamed.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Failed to commit Excel: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a minimal .xlsx with the given headers + string data rows.
    fn write_xlsx<S: AsRef<str>>(path: &Path, headers: &[&str], rows: &[Vec<S>]) {
        let mut wb = XlsxWorkbook::new();
        let ws = wb.add_worksheet();
        for (c, h) in headers.iter().enumerate() {
            ws.write(0, c as u16, *h).unwrap();
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write((r + 1) as u32, c as u16, val.as_ref()).unwrap();
            }
        }
        wb.save(path).unwrap();
    }

    /// A firmware-valid 20-char UUID derived from a short tag (padded with 'x').
    fn vu(tag: &str) -> String {
        format!("{tag:x<20}")
    }
    /// A firmware-valid 32-char AuthKey derived from a short tag (padded with 'k').
    fn vk(tag: &str) -> String {
        format!("{tag:k<32}")
    }
    /// Build a `Vec<String>` row from string-ish parts (ergonomic fixtures).
    fn row(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unix_to_utc_epoch() {
        assert_eq!(unix_to_utc(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn unix_to_utc_known_dates() {
        // Exact day boundary.
        assert_eq!(unix_to_utc(1_609_459_200), (2021, 1, 1, 0, 0, 0));
        // Day after a leap day — exercises month_days[1] == 29.
        assert_eq!(unix_to_utc(1_583_020_800), (2020, 3, 1, 0, 0, 0));
    }

    #[test]
    fn is_leap_cases() {
        assert!(is_leap(2000)); // divisible by 400
        assert!(!is_leap(1900)); // divisible by 100, not 400
        assert!(is_leap(2024)); // divisible by 4
        assert!(!is_leap(2023));
    }

    #[test]
    fn allocator_round_trip_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY", "STATUS", "MAC", "TIMESTAMP"],
            &[
                row(&[&vu("uuid-a"), &vk("key-a"), "", "", ""]),
                row(&[&vu("uuid-b"), &vk("key-b"), "", "", ""]),
                row(&[
                    &vu("uuid-c"),
                    &vk("key-c"),
                    "DONE",
                    "AA:BB",
                    "2024-01-01T00:00:00Z",
                ]),
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let s = alloc.stats();
        // uuid-c is DONE → used=1, remaining=2
        assert_eq!((s.total, s.used, s.remaining), (3, 1, 2));

        // Allocate returns the first Available row; MacRead counts as in_progress.
        let row_a = alloc.allocate_row().unwrap();
        assert_eq!(row_a.uuid, vu("uuid-a"));
        let s = alloc.stats();
        assert_eq!((s.used, s.remaining, s.in_progress), (1, 1, 1));

        let row_b = alloc.allocate_row().unwrap();
        assert_eq!(row_b.uuid, vu("uuid-b"));
        assert_eq!(alloc.stats().remaining, 0);

        // Exhausted — no Available rows left.
        assert!(alloc.allocate_row().is_err());

        // update_row_state to AuthVerified marks as used and persists (+ .bak).
        alloc
            .update_row_state(
                row_a.row_idx,
                "11:22:33:44:55:66",
                RowStatus::AuthVerified,
                Some("auth_verified"),
                None,
            )
            .unwrap();
        assert_eq!(alloc.stats().used, 2);
        assert!(path.with_extension("xlsx.bak").exists());

        // Reload from disk: uuid-a's AUTHVERIFIED status and MAC survived the round trip.
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        let s = reloaded.stats();
        assert_eq!((s.total, s.used), (3, 2));
        let found = reloaded.find_by_mac("11:22:33:44:55:66").unwrap();
        assert_eq!(found.1, vu("uuid-a"));
    }

    /// Legacy Excel produced by an older tyutool build may carry OTPLOCKED rows.
    /// OTP lock is gone, but such rows must still parse and count as authorized.
    #[test]
    fn legacy_otplocked_rows_still_read_as_authorized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY", "STATUS", "MAC", "TIMESTAMP"],
            &[
                row(&[
                    &vu("uuid-a"),
                    &vk("key-a"),
                    "OTPLOCKED",
                    "AA:BB:CC:DD:EE:FF",
                    "",
                ]),
                row(&[&vu("uuid-b"), &vk("key-b"), "", "", ""]),
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let s = alloc.stats();
        // OTPLOCKED row counts as used; the empty row remains available.
        assert_eq!((s.total, s.used, s.remaining), (2, 1, 1));
        let found = alloc.find_by_mac("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(found.1, vu("uuid-a"));
    }

    #[test]
    fn load_rejects_missing_uuid_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.xlsx");
        write_xlsx(&path, &["AUTHKEY", "STATUS"], &[vec!["key-a", ""]]);
        let err = ExcelRowAllocator::load(&path).err().unwrap();
        assert!(err.contains("UUID"), "unexpected error: {err}");
    }

    #[test]
    fn update_row_state_persists_and_find_by_mac_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[
                row(&[&vu("uuid-a"), &vk("key-a")]),
                row(&[&vu("uuid-b"), &vk("key-b")]),
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();

        // 初始时按 MAC 找不到
        assert!(alloc.find_by_mac("AA:BB:CC:DD:EE:FF").is_none());

        // 分配行 0，绑定 MAC
        let row = alloc.allocate_row().unwrap();
        assert_eq!(row.row_idx, 0);
        alloc
            .update_row_state(
                row.row_idx,
                "AA:BB:CC:DD:EE:FF",
                RowStatus::MacRead,
                Some("mac_read"),
                None,
            )
            .unwrap();

        // 现在可以按 MAC 找到
        let found = alloc.find_by_mac("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(found.0, 0);
        assert_eq!(found.1, vu("uuid-a"));

        // 继续推进状态
        alloc
            .update_row_state(
                0,
                "AA:BB:CC:DD:EE:FF",
                RowStatus::AuthWritten,
                Some("auth_written"),
                None,
            )
            .unwrap();
        alloc
            .update_row_state(
                0,
                "AA:BB:CC:DD:EE:FF",
                RowStatus::AuthVerified,
                Some("auth_verified"),
                None,
            )
            .unwrap();

        // stats: 1 used, 1 remaining
        let s = alloc.stats();
        assert_eq!(s.used, 1);
        assert_eq!(s.remaining, 1);
        assert_eq!(s.in_progress, 0);

        // 重载后状态保持
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        let found2 = reloaded.find_by_mac("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(found2.0, 0);
        assert_eq!(reloaded.stats().used, 1);
    }

    #[test]
    fn update_row_state_with_error_preserves_step() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[row(&[&vu("uuid-a"), &vk("key-a")])],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let alloc_row = alloc.allocate_row().unwrap();
        alloc
            .update_row_state(
                alloc_row.row_idx,
                "11:22:33:44:55:66",
                RowStatus::AuthWritten,
                Some("auth_written"),
                Some("verify: no response"),
            )
            .unwrap();

        // 重载，错误信息保留；状态仍是 AuthWritten（不是 Available）
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        // find_by_mac 仍能找到
        assert!(reloaded.find_by_mac("11:22:33:44:55:66").is_some());
        // stats: in_progress = 1（AuthWritten 是 in_progress）
        assert_eq!(reloaded.stats().in_progress, 1);
        assert_eq!(reloaded.stats().remaining, 0);
    }

    #[test]
    fn confirm_existing_uuid_claims_available_row_and_blocks_reallocation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[
                row(&[&vu("uuid-a"), &vk("key-a")]),
                row(&[&vu("uuid-b"), &vk("key-b")]),
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();

        // Device A already carries uuid-a (Skip path). Claim that row for A's MAC.
        alloc
            .confirm_existing_uuid(&vu("uuid-a"), "AA:AA:AA:AA:AA:AA")
            .unwrap();
        let s = alloc.stats();
        assert_eq!(s.used, 1);
        assert_eq!(s.remaining, 1);
        // The claimed row is now bound to A's MAC.
        let found = alloc.find_by_mac("AA:AA:AA:AA:AA:AA").unwrap();
        assert_eq!(found.1, vu("uuid-a"));

        // A later blank device must NOT get uuid-a again — it gets uuid-b.
        let alloc_row = alloc.allocate_row().unwrap();
        assert_eq!(alloc_row.uuid, vu("uuid-b"));

        // Persisted across reload.
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        assert!(reloaded.find_by_mac("AA:AA:AA:AA:AA:AA").is_some());
    }

    #[test]
    fn confirm_existing_uuid_is_noop_for_unknown_or_claimed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[row(&[&vu("uuid-a"), &vk("key-a")])],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();

        // Unknown UUID → no error, nothing claimed.
        alloc.confirm_existing_uuid("uuid-x", "AA:BB").unwrap();
        assert_eq!(alloc.stats().used, 0);
        assert_eq!(alloc.stats().remaining, 1);

        // Bind uuid-a to one device, then a second device reporting the same
        // UUID must not steal/overwrite the already-bound row.
        alloc.confirm_existing_uuid(&vu("uuid-a"), "11:11").unwrap();
        alloc.confirm_existing_uuid(&vu("uuid-a"), "22:22").unwrap();
        let found = alloc.find_by_mac("11:11").unwrap();
        assert_eq!(found.1, vu("uuid-a"));
        assert!(alloc.find_by_mac("22:22").is_none());
        assert_eq!(alloc.stats().used, 1);
    }

    #[test]
    fn load_with_duplicate_mac_does_not_error_and_first_row_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        // Two rows carry the same MAC (e.g. an operator copy/paste error).
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY", "STATUS", "MAC"],
            &[
                row(&[&vu("uuid-a"), &vk("key-a"), "DONE", "AA:BB:CC:DD:EE:FF"]),
                row(&[&vu("uuid-b"), &vk("key-b"), "DONE", "AA:BB:CC:DD:EE:FF"]),
            ],
        );
        // Load succeeds (the duplicate only triggers a log::warn).
        let alloc = ExcelRowAllocator::load(&path).unwrap();
        // find_by_mac returns the first matching row deterministically.
        let found = alloc.find_by_mac("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(found.0, 0);
        assert_eq!(found.1, vu("uuid-a"));
    }

    #[test]
    fn invalid_length_rows_are_counted_and_never_allocated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[
                row(&["short-uuid", &vk("key-a")]),  // bad UUID length
                row(&[&vu("uuid-b"), "shortkey"]),   // bad AuthKey length
                row(&[&vu("uuid-c"), &vk("key-c")]), // valid
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let s = alloc.stats();
        assert_eq!(s.total, 3);
        assert_eq!(s.invalid, 2);
        assert_eq!(s.remaining, 1, "only the one valid row counts as remaining");

        // Allocation skips the two malformed rows and hands out the valid one.
        let first = alloc.allocate_row().unwrap();
        assert_eq!(first.uuid, vu("uuid-c"));
        // Now exhausted — the invalid rows are never handed out.
        assert!(alloc.allocate_row().is_err());
    }

    #[test]
    fn save_is_atomic_and_writes_prev_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[row(&[&vu("uuid-a"), &vk("key-a")])],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let r = alloc.allocate_row().unwrap();
        // First save creates the pristine .bak; no temp file is left behind.
        alloc
            .update_row_state(
                r.row_idx,
                "AA:BB",
                RowStatus::MacRead,
                Some("mac_read"),
                None,
            )
            .unwrap();
        assert!(path.with_extension("xlsx.bak").exists());
        assert!(
            !path.with_extension("xlsx.tmp").exists(),
            "temp file must be renamed away"
        );

        // Second save rolls the previous good copy into .prev.bak.
        alloc
            .update_row_state(
                r.row_idx,
                "AA:BB",
                RowStatus::AuthVerified,
                Some("auth_verified"),
                None,
            )
            .unwrap();
        assert!(path.with_extension("xlsx.prev.bak").exists());
        // Main file is valid and reloadable after the atomic rename.
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        assert_eq!(reloaded.stats().used, 1);
    }

    fn lock_fixture(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = dir.path().join("auth.xlsx");
        write_xlsx(
            &path,
            &["UUID", "AUTHKEY"],
            &[row(&[&vu("uuid-a"), &vk("key-a")])],
        );
        path
    }

    #[cfg(windows)]
    #[test]
    fn load_locked_fails_when_file_held_without_write_share() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = lock_fixture(&dir);
        // Simulate Excel: hold the file open denying write sharing.
        let _excel = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        assert!(matches!(
            ExcelRowAllocator::load_locked(&path),
            Err(e) if e == "excel.locked"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn load_locked_blocks_second_locked_load_until_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_fixture(&dir);
        let first = ExcelRowAllocator::load_locked(&path).unwrap();
        assert!(matches!(
            ExcelRowAllocator::load_locked(&path),
            Err(e) if e == "excel.locked"
        ));
        drop(first);
        assert!(ExcelRowAllocator::load_locked(&path).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn plain_load_succeeds_while_locked() {
        // validate_excel_cmd must keep working during an active batch.
        let dir = tempfile::tempdir().unwrap();
        let path = lock_fixture(&dir);
        let _locked = ExcelRowAllocator::load_locked(&path).unwrap();
        let plain = ExcelRowAllocator::load(&path).unwrap();
        assert_eq!(plain.stats().total, 1);
    }

    #[cfg(windows)]
    #[test]
    fn save_under_lock_keeps_lock_and_writes_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_fixture(&dir);
        let alloc = ExcelRowAllocator::load_locked(&path).unwrap();
        let r = alloc.allocate_row().unwrap();
        alloc
            .update_row_state(
                r.row_idx,
                "AA:BB",
                RowStatus::MacRead,
                Some("mac_read"),
                None,
            )
            .unwrap();
        alloc
            .update_row_state(
                r.row_idx,
                "AA:BB",
                RowStatus::AuthVerified,
                Some("auth_verified"),
                None,
            )
            .unwrap();
        // Backups written via the share-compatible copy despite the lock.
        assert!(path.with_extension("xlsx.bak").exists());
        assert!(path.with_extension("xlsx.prev.bak").exists());
        assert!(!path.with_extension("xlsx.tmp").exists());
        // Lock survived the drop→rename→reopen cycle in save_workbook.
        assert!(matches!(
            ExcelRowAllocator::load_locked(&path),
            Err(e) if e == "excel.locked"
        ));
        // Read path still sees persisted state.
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        assert_eq!(reloaded.stats().used, 1);
    }

    #[test]
    fn release_lock_allows_relock() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_fixture(&dir);
        let first = ExcelRowAllocator::load_locked(&path).unwrap();
        first.release_lock();
        // Release is idempotent and frees the file for a new locked load
        // even while the first allocator is still alive.
        first.release_lock();
        let second = ExcelRowAllocator::load_locked(&path).unwrap();
        assert_eq!(second.stats().total, 1);
    }
}
