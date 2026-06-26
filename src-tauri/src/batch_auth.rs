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
    total_cols: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowStatus {
    Available,
    Allocated,
    Used,
}

#[derive(Debug, Clone)]
struct RowData {
    uuid: String,
    authkey: String,
    status: RowStatus,
    mac: Option<String>,
    timestamp: Option<String>,
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
        let total_cols = header_strings.len();

        let header = HeaderInfo {
            uuid_col,
            authkey_col,
            status_col,
            mac_col,
            timestamp_col,
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
            let mac = mac_col.map(|i| get(i)).filter(|s| !s.is_empty());
            let timestamp = timestamp_col.map(|i| get(i)).filter(|s| !s.is_empty());

            let status = if status_str.to_uppercase() == "USED" {
                RowStatus::Used
            } else {
                RowStatus::Available
            };

            let known: HashSet<usize> = [
                Some(uuid_col),
                Some(authkey_col),
                status_col,
                mac_col,
                timestamp_col,
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
            .filter(|r| r.status == RowStatus::Used)
            .count();
        let remaining = state
            .rows
            .iter()
            .filter(|r| r.status == RowStatus::Available)
            .count();
        ExcelStats {
            total,
            used,
            remaining,
        }
    }

    pub fn allocate_row(&self) -> Result<ExcelRow, String> {
        let mut state = self.state.lock().unwrap();
        for (idx, row) in state.rows.iter_mut().enumerate() {
            if row.status == RowStatus::Available {
                row.status = RowStatus::Allocated;
                return Ok(ExcelRow {
                    row_idx: idx,
                    uuid: row.uuid.clone(),
                    authkey: row.authkey.clone(),
                });
            }
        }
        Err("Authorization codes exhausted — no available rows in Excel".into())
    }

    pub fn release_row(&self, row_idx: usize) {
        let mut state = self.state.lock().unwrap();
        if let Some(row) = state.rows.get_mut(row_idx) {
            if row.status == RowStatus::Allocated {
                row.status = RowStatus::Available;
            }
        }
    }

    pub fn confirm_row(&self, row_idx: usize, mac: String) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();

        if !state.backed_up {
            let bak = state.path.with_extension("xlsx.bak");
            if !bak.exists() {
                std::fs::copy(&state.path, &bak).ok();
            }
            state.backed_up = true;
        }

        if let Some(row) = state.rows.get_mut(row_idx) {
            row.status = RowStatus::Used;
            row.mac = Some(mac);
            row.timestamp = Some(utc_now_iso8601());
        }

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

    // Data rows
    for (i, row) in state.rows.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write(r, h.uuid_col as u16, row.uuid.as_str())
            .map_err(|e| e.to_string())?;
        ws.write(r, h.authkey_col as u16, row.authkey.as_str())
            .map_err(|e| e.to_string())?;
        let status_str = if row.status == RowStatus::Used {
            "USED"
        } else {
            ""
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
                vec!["uuid-c", "key-c", "USED", "AA:BB", "2024-01-01T00:00:00Z"],
            ],
        );

        let alloc = ExcelRowAllocator::load(&path).unwrap();
        let s = alloc.stats();
        assert_eq!((s.total, s.used, s.remaining), (3, 1, 2));

        // Allocate returns the first Available row; Allocated counts as neither
        // used nor remaining, so remaining drops while used stays.
        let row_a = alloc.allocate_row().unwrap();
        assert_eq!(row_a.uuid, "uuid-a");
        let s = alloc.stats();
        assert_eq!((s.used, s.remaining), (1, 1));

        let row_b = alloc.allocate_row().unwrap();
        assert_eq!(row_b.uuid, "uuid-b");
        assert_eq!(alloc.stats().remaining, 0);

        // Exhausted — no Available rows left.
        assert!(alloc.allocate_row().is_err());

        // Releasing returns an Allocated row to Available.
        alloc.release_row(row_b.row_idx);
        assert_eq!(alloc.stats().remaining, 1);

        // Confirm marks Used and persists to disk (+ creates a .bak backup).
        alloc
            .confirm_row(row_a.row_idx, "11:22:33:44:55:66".into())
            .unwrap();
        assert_eq!(alloc.stats().used, 2);
        assert!(path.with_extension("xlsx.bak").exists());

        // Reload from disk: uuid-a's USED status and MAC survived the round trip.
        let reloaded = ExcelRowAllocator::load(&path).unwrap();
        let s = reloaded.stats();
        assert_eq!((s.total, s.used), (3, 2));
    }

    #[test]
    fn load_rejects_missing_uuid_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.xlsx");
        write_xlsx(&path, &["AUTHKEY", "STATUS"], &[vec!["key-a", ""]]);
        let err = ExcelRowAllocator::load(&path).err().unwrap();
        assert!(err.contains("UUID"), "unexpected error: {err}");
    }
}
