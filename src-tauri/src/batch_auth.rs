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
    /// auth-otp-lock 成功（仅在启用 lock_otp 时出现）。
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
}

struct AllocatorState {
    path: PathBuf,
    header: HeaderInfo,
    header_raw: Vec<String>,
    rows: Vec<RowData>,
    backed_up: bool,
}

pub struct ExcelRowAllocator {
    state: Mutex<AllocatorState>,
}

impl ExcelRowAllocator {
    pub fn path_matches(&self, path: &Path) -> bool {
        self.state.lock().map(|s| s.path == path).unwrap_or(false)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
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
            });
        }

        Ok(Self {
            state: Mutex::new(AllocatorState {
                path: path.to_owned(),
                header,
                header_raw: header_strings,
                rows,
                backed_up: false,
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
            .filter(|r| r.status == RowStatus::Available)
            .count();
        ExcelStats {
            total,
            used,
            in_progress,
            remaining,
        }
    }

    pub fn allocate_row(&self) -> Result<ExcelRow, String> {
        let mut state = self.state.lock().unwrap();
        for (idx, row) in state.rows.iter_mut().enumerate() {
            if row.status == RowStatus::Available {
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
        {
            let mut state = self.state.lock().unwrap();

            if !state.backed_up {
                let bak = state.path.with_extension("xlsx.bak");
                if !bak.exists() {
                    std::fs::copy(&state.path, &bak).ok();
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
        }
        let state = self.state.lock().unwrap();
        save_workbook(&state)
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

fn save_workbook(state: &AllocatorState) -> Result<(), String> {
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

    wb.save(&state.path)
        .map_err(|e| format!("Failed to save Excel: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a minimal .xlsx with the given headers + string data rows.
    fn write_xlsx(path: &Path, headers: &[&str], rows: &[Vec<&str>]) {
        let mut wb = XlsxWorkbook::new();
        let ws = wb.add_worksheet();
        for (c, h) in headers.iter().enumerate() {
            ws.write(0, c as u16, *h).unwrap();
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write((r + 1) as u32, c as u16, *val).unwrap();
            }
        }
        wb.save(path).unwrap();
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
                vec!["uuid-a", "key-a", "", "", ""],
                vec!["uuid-b", "key-b", "", "", ""],
                vec!["uuid-c", "key-c", "DONE", "AA:BB", "2024-01-01T00:00:00Z"],
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let s = alloc.stats();
        // uuid-c is DONE → used=1, remaining=2
        assert_eq!((s.total, s.used, s.remaining), (3, 1, 2));

        // Allocate returns the first Available row; MacRead counts as in_progress.
        let row_a = alloc.allocate_row().unwrap();
        assert_eq!(row_a.uuid, "uuid-a");
        let s = alloc.stats();
        assert_eq!((s.used, s.remaining, s.in_progress), (1, 1, 1));

        let row_b = alloc.allocate_row().unwrap();
        assert_eq!(row_b.uuid, "uuid-b");
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
        assert_eq!(found.1, "uuid-a");
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
            &[vec!["uuid-a", "key-a"], vec!["uuid-b", "key-b"]],
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
        assert_eq!(found.1, "uuid-a");

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
        write_xlsx(&path, &["UUID", "AUTHKEY"], &[vec!["uuid-a", "key-a"]]);

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let row = alloc.allocate_row().unwrap();
        alloc
            .update_row_state(
                row.row_idx,
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
}
